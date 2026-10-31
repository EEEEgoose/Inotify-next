//! Runtime settings that administrators change from the web UI, stored as one JSON row.

use crate::db;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

const KEY: &str = "app_settings";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Proxy for outgoing HTTP requests, e.g. `http://127.0.0.1:7890` or `socks5://...`.
    pub proxy: String,
    /// Cloudflare Turnstile on the login page.
    pub turnstile_enabled: bool,
    pub turnstile_site_key: String,
    pub turnstile_secret_key: String,
}

impl Settings {
    pub fn turnstile_ready(&self) -> bool {
        self.turnstile_enabled && !self.turnstile_site_key.is_empty() && !self.turnstile_secret_key.is_empty()
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
