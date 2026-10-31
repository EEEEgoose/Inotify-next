//! Outbound HTTP: one shared client for fixed API hosts, and per-request clients for
//! user-supplied URLs that refuse private network targets (SSRF guard). An optional proxy,
//! changeable at runtime, applies to both.

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
    inner: RwLock<Inner>,
}

struct Inner {
    proxy: Option<reqwest::Proxy>,
    shared: reqwest::Client,
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
        let proxy = parse_proxy(proxy)?;
        let shared = builder(&proxy).build()?;
        Ok(Self {
            allow_private,
            inner: RwLock::new(Inner { proxy, shared }),
        })
    }

    /// Replaces the proxy (empty string: none) for all later requests.
    pub fn set_proxy(&self, proxy: &str) -> anyhow::Result<()> {
        let proxy = parse_proxy(proxy)?;
        let shared = builder(&proxy).build()?;
        *self.inner.write().unwrap() = Inner { proxy, shared };
        Ok(())
    }

    /// Client for fixed, well-known API hosts.
    pub fn client(&self) -> reqwest::Client {
        self.inner.read().unwrap().shared.clone()
    }

    /// Returns a client for a user-supplied `url`. Unless private targets are allowed, every
    /// address the host resolves to must be public, and the client is pinned to the checked
    /// address so a second DNS lookup cannot point it elsewhere. Redirects are never followed.
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
        let proxy = self.inner.read().unwrap().proxy.clone();
        Ok(builder(&proxy).resolve(host, addr).build()?)
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
            if let Some(bad) = addrs.iter().find(|a| !is_public(a.ip())) {
                bail!("{host} resolves to non-public address {}", bad.ip());
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
        || (a == 198 && (b == 18 || b == 19)) // benchmarking
        || a >= 240)
}

fn is_public_v6(ip: Ipv6Addr) -> bool {
    let first = ip.segments()[0];
    !(ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        || (first & 0xfe00) == 0xfc00 // unique local
        || (first & 0xffc0) == 0xfe80) // link local
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
        ] {
            assert!(!is_public(private.parse().unwrap()), "{private} should be private");
        }
        for public in ["1.1.1.1", "8.8.8.8", "2606:4700::1111"] {
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
