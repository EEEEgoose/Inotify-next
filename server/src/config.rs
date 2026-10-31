use std::{env, path::PathBuf};

#[derive(Clone, Debug)]
pub struct Config {
    pub listen: String,
    pub data_dir: PathBuf,
    /// Let webhook and email channels reach loopback / LAN / link-local addresses.
    pub allow_private_targets: bool,
    pub send_concurrency: usize,
    pub endpoints: Endpoints,
}

/// Base URLs of third-party APIs; only tests point them elsewhere.
#[derive(Clone, Debug)]
pub struct Endpoints {
    pub apns: String,
    pub telegram: String,
    pub wecom: String,
    pub github_web: String,
    pub github_api: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            apns: "https://api.push.apple.com".into(),
            telegram: "https://api.telegram.org".into(),
            wecom: "https://qyapi.weixin.qq.com".into(),
            github_web: "https://github.com".into(),
            github_api: "https://api.github.com".into(),
        }
    }
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            listen: env::var("INOTIFY_LISTEN").unwrap_or_else(|_| "0.0.0.0:8000".into()),
            data_dir: env::var("INOTIFY_DATA_DIR").unwrap_or_else(|_| "data".into()).into(),
            allow_private_targets: env::var("INOTIFY_ALLOW_PRIVATE_TARGETS")
                .is_ok_and(|v| matches!(v.as_str(), "1" | "true")),
            send_concurrency: env::var("INOTIFY_SEND_CONCURRENCY")
                .ok()
                .and_then(|v| v.parse().ok())
                .filter(|&n| n > 0)
                .unwrap_or(8),
            endpoints: Endpoints::default(),
        }
    }
}
