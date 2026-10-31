use std::{env, path::PathBuf};

#[derive(Clone, Debug)]
pub struct Config {
    pub listen: String,
    pub data_dir: PathBuf,
    /// Let webhook and email channels reach loopback / LAN / link-local addresses.
    pub allow_private_targets: bool,
    pub send_concurrency: usize,
    /// Behind a reverse proxy: take the client IP from `X-Forwarded-For`. Off, it is ignored.
    pub trust_proxy: bool,
    /// Skip the Turnstile check on login even when it is enabled in the settings, to recover from
    /// a wrong site key or an unreachable Cloudflare.
    pub disable_turnstile: bool,
    pub endpoints: Endpoints,
}

/// Base URLs of third-party APIs; only tests point them elsewhere.
#[derive(Clone, Debug)]
pub struct Endpoints {
    pub apns: String,
    pub telegram: String,
    pub wecom: String,
    pub turnstile: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            apns: "https://api.push.apple.com".into(),
            telegram: "https://api.telegram.org".into(),
            wecom: "https://qyapi.weixin.qq.com".into(),
            turnstile: "https://challenges.cloudflare.com".into(),
        }
    }
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            listen: env::var("INOTIFY_LISTEN").unwrap_or_else(|_| "0.0.0.0:8000".into()),
            data_dir: env::var("INOTIFY_DATA_DIR").unwrap_or_else(|_| "data".into()).into(),
            allow_private_targets: flag("INOTIFY_ALLOW_PRIVATE_TARGETS"),
            send_concurrency: env::var("INOTIFY_SEND_CONCURRENCY")
                .ok()
                .and_then(|v| v.parse().ok())
                .filter(|&n| n > 0)
                .unwrap_or(8),
            trust_proxy: flag("INOTIFY_TRUST_PROXY"),
            disable_turnstile: flag("INOTIFY_DISABLE_TURNSTILE"),
            endpoints: Endpoints::default(),
        }
    }
}

fn flag(name: &str) -> bool {
    env::var(name).is_ok_and(|v| matches!(v.as_str(), "1" | "true"))
}
