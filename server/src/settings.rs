//! Runtime settings that administrators change from the web UI, stored as one JSON row.

use crate::db;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

const KEY: &str = "app_settings";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub github_enabled: bool,
    pub github_client_id: String,
    pub github_client_secret: String,
    /// Create an account for a GitHub user who has none yet.
    pub github_allow_signup: bool,
    /// Proxy for outgoing HTTP requests, e.g. `http://127.0.0.1:7890` or `socks5://...`.
    pub proxy: String,
    /// External address, e.g. `https://inotify.example.com`; used for the OAuth callback.
    pub public_url: String,
}

impl Settings {
    pub fn github_ready(&self) -> bool {
        self.github_enabled && !self.github_client_id.is_empty() && !self.github_client_secret.is_empty()
    }
}

pub async fn load(pool: &SqlitePool) -> anyhow::Result<Settings> {
    Ok(match db::get_setting(pool, KEY).await? {
        Some(json) => serde_json::from_str(&json)?,
        None => Settings::default(),
    })
}

pub async fn save(pool: &SqlitePool, settings: &Settings) -> anyhow::Result<()> {
    db::set_setting(pool, KEY, &serde_json::to_string(settings)?).await?;
    Ok(())
}
