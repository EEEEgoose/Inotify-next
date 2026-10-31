//! Outbound HTTP: one shared client for fixed API hosts, and per-request clients for
//! user-supplied URLs that refuse private network targets (SSRF guard). An optional proxy,
//! changeable at runtime, applies to the fixed hosts; user-supplied URLs only use it when private
//! targets are allowed, because the proxy would resolve the host again and could reach anything.

use anyhow::{bail, Context};
use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::RwLock,
    time::Duration,
};
use url::Url;

pub const TIMEOUT: Duration = Duration::from_secs(15);

pub struct Net {
    allow_private: bool,
    shared: RwLock<reqwest::Client>,
}

fn builder(proxy: &Option<reqwest::Proxy>) -> reqwest::ClientBuilder {
    let builder = reqwest::Client::builder()
        .timeout(TIMEOUT)
        .redirect(reqwest::redirect::Policy::none());
    match proxy {
        Some(p) => builder.proxy(p.clone()),
        None => builder,
    }
}

fn parse_proxy(proxy: &str) -> anyhow::Result<Option<reqwest::Proxy>> {
    match proxy.trim() {
        "" => Ok(None),
        url => Ok(Some(reqwest::Proxy::all(url).context("invalid proxy URL")?)),
    }
}

impl Net {
    pub fn new(allow_private: bool, proxy: &str) -> anyhow::Result<Self> {
        let shared = builder(&parse_proxy(proxy)?).build()?;
        Ok(Self {
            allow_private,
            shared: RwLock::new(shared),
        })
    }

    /// Replaces the proxy (empty string: none) for all later requests.
    pub fn set_proxy(&self, proxy: &str) -> anyhow::Result<()> {
        let shared = builder(&parse_proxy(proxy)?).build()?;
        *self.shared.write().unwrap() = shared;
        Ok(())
    }

    /// Client for fixed, well-known API hosts.
    pub fn client(&self) -> reqwest::Client {
        self.shared.read().unwrap().clone()
    }

    /// Returns a client for a user-supplied `url`. Unless private targets are allowed, every
    /// address the host resolves to must be public, and the client connects directly (no proxy),
    /// pinned to the checked address so a second DNS lookup cannot point it elsewhere.
    /// Redirects are never followed.
    pub async fn client_for(&self, url: &Url) -> anyhow::Result<reqwest::Client> {
        if !matches!(url.scheme(), "http" | "https") {
            bail!("only http and https URLs are allowed");
        }
        if self.allow_private {
            return Ok(self.client());
        }
        let host = url.host_str().context("URL has no host")?;
        let port = url.port_or_known_default().context("URL has no port")?;
        let addr = self.resolve(host, port).await?;
        // `no_proxy` also ignores HTTP(S)_PROXY from the environment.
        Ok(builder(&None).no_proxy().resolve(host, addr).build()?)
    }

    /// Resolves `host`, refusing non-public addresses unless private targets are allowed.
    pub async fn resolve(&self, host: &str, port: u16) -> anyhow::Result<SocketAddr> {
        let lookup_host = host.trim_start_matches('[').trim_end_matches(']');
        let addrs: Vec<SocketAddr> = tokio::net::lookup_host((lookup_host, port))
            .await
            .with_context(|| format!("resolve {host}"))?
            .collect();
        let first = *addrs.first().with_context(|| format!("{host} did not resolve"))?;
        if !self.allow_private {
            // The address itself is not reported: it would map the internal network.
            if addrs.iter().any(|a| !is_public(a.ip())) {
                bail!("{host} resolves to a non-public address");
            }
        }
        Ok(first)
    }
}

pub fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_public_v4(v4),
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => is_public_v4(v4),
            None => is_public_v6(v6),
        },
    }
}

fn is_public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, ..] = ip.octets();
    !(ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_unspecified()
        || ip.is_multicast()
        || a == 0
        || (a == 100 && (64..128).contains(&b)) // carrier-grade NAT
        || (a == 192 && b == 0 && ip.octets()[2] == 0) // IETF protocol assignments
        || (a == 198 && (b == 18 || b == 19)) // benchmarking
        || a >= 240)
}

fn is_public_v6(ip: Ipv6Addr) -> bool {
    let s = ip.segments();
    let v4 = |hi: u16, lo: u16| Ipv4Addr::new((hi >> 8) as u8, hi as u8, (lo >> 8) as u8, lo as u8);
    // Addresses that carry an IPv4 address are judged by it: IPv4-compatible (also covers `::`
    // and `::1`), NAT64 and 6to4.
    if s[..6] == [0; 6] || s[..6] == [0x64, 0xff9b, 0, 0, 0, 0] {
        return is_public_v4(v4(s[6], s[7]));
    }
    if s[0] == 0x2002 {
        return is_public_v4(v4(s[1], s[2]));
    }
    !(ip.is_multicast()
        || (s[0] & 0xfe00) == 0xfc00 // unique local
        || (s[0] & 0xffc0) == 0xfe80 // link local
        || (s[0] & 0xffc0) == 0xfec0 // site local (deprecated)
        || (s[0] == 0x64 && s[1] == 0xff9b && s[2] == 1) // local-use NAT64
        || (s[0] == 0x2001 && s[1] == 0) // Teredo
        || (s[0] == 0x2001 && s[1] == 0xdb8) // documentation
        || (s[0] == 0x100 && s[1..4] == [0, 0, 0])) // discard
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_addresses() {
        for private in [
            "127.0.0.1",
            "10.1.2.3",
            "192.168.1.1",
            "172.16.0.1",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "::1",
            "fd00::1",
            "fe80::1",
            "::ffff:127.0.0.1",
            "::",
            "::127.0.0.1",
            "64:ff9b::a00:1",
            "64:ff9b:1::1",
            "2002:c0a8:101::1",
            "2001:0:4136:e378::1",
            "fec0::1",
            "192.0.0.8",
        ] {
            assert!(!is_public(private.parse().unwrap()), "{private} should be private");
        }
        for public in [
            "1.1.1.1",
            "8.8.8.8",
            "2606:4700::1111",
            "64:ff9b::808:808",
            "2002:808:808::1",
        ] {
            assert!(is_public(public.parse().unwrap()), "{public} should be public");
        }
    }

    #[tokio::test]
    async fn refuses_private_targets() {
        let net = Net::new(false, "").unwrap();
        for url in [
            "http://127.0.0.1:8080/x",
            "http://localhost/x",
            "http://[::1]/x",
            "file:///etc/passwd",
        ] {
            assert!(
                net.client_for(&Url::parse(url).unwrap()).await.is_err(),
                "{url} should be refused"
            );
        }
    }
}
