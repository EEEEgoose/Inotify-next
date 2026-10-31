use anyhow::Context;
use rand::{distributions::Alphanumeric, Rng};
use serde::Serialize;
use sqlx::{
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
    SqlitePool,
};
use std::{path::Path, str::FromStr};

pub async fn connect(data_dir: &Path) -> anyhow::Result<SqlitePool> {
    std::fs::create_dir_all(data_dir).with_context(|| format!("create data dir {}", data_dir.display()))?;
    let url = format!("sqlite://{}", data_dir.join("inotify.db").display());
    let opts = SqliteConnectOptions::from_str(&url)?
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new().max_connections(8).connect_with(opts).await?;
    migrate(&pool).await?;
    Ok(pool)
}

/// A single-connection in-memory database, for tests.
pub async fn connect_memory() -> anyhow::Result<SqlitePool> {
    let opts = SqliteConnectOptions::from_str("sqlite::memory:")?.foreign_keys(true);
    let pool = SqlitePoolOptions::new().max_connections(1).connect_with(opts).await?;
    migrate(&pool).await?;
    Ok(pool)
}

async fn migrate(pool: &SqlitePool) -> anyhow::Result<()> {
    sqlx::migrate!("./migrations").run(pool).await?;
    Ok(())
}

pub fn random_token(len: usize) -> String {
    rand::rngs::OsRng
        .sample_iter(&Alphanumeric)
        .take(len)
        .map(char::from)
        .collect()
}

pub async fn get_setting(pool: &SqlitePool, key: &str) -> sqlx::Result<Option<String>> {
    sqlx::query_scalar("SELECT value FROM settings WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await
}

pub async fn set_setting(pool: &SqlitePool, key: &str, value: &str) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(key)
    .bind(value)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn jwt_secret(pool: &SqlitePool) -> anyhow::Result<String> {
    if let Some(secret) = get_setting(pool, "jwt_secret").await? {
        return Ok(secret);
    }
    let secret = random_token(64);
    set_setting(pool, "jwt_secret", &secret).await?;
    Ok(secret)
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct User {
    pub id: i64,
    pub username: String,
    /// Serialized only as `has_password: bool`.
    #[serde(rename = "has_password", serialize_with = "serialize_is_some")]
    pub password_hash: Option<String>,
    pub is_admin: bool,
    pub active: bool,
    pub send_token: String,
    pub github_id: Option<i64>,
    pub github_login: Option<String>,
    pub avatar_url: Option<String>,
    pub created_at: String,
}

impl User {
    pub fn has_password(&self) -> bool {
        self.password_hash.is_some()
    }
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Channel {
    pub id: i64,
    pub user_id: i64,
    pub kind: String,
    pub name: String,
    pub key: String,
    #[serde(serialize_with = "serialize_json_text")]
    pub config: String,
    pub active: bool,
    pub created_at: String,
    pub updated_at: String,
}

fn serialize_is_some<S: serde::Serializer>(value: &Option<String>, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_bool(value.is_some())
}

fn serialize_json_text<S: serde::Serializer>(text: &str, s: S) -> Result<S::Ok, S::Error> {
    let value: serde_json::Value = serde_json::from_str(text).unwrap_or_default();
    value.serialize(s)
}

pub async fn create_user(
    pool: &SqlitePool,
    username: &str,
    password_hash: Option<&str>,
    is_admin: bool,
) -> sqlx::Result<User> {
    sqlx::query_as("INSERT INTO users (username, password_hash, is_admin, send_token) VALUES (?, ?, ?, ?) RETURNING *")
        .bind(username)
        .bind(password_hash)
        .bind(is_admin)
        .bind(random_token(32))
        .fetch_one(pool)
        .await
}

pub async fn user_by_id(pool: &SqlitePool, id: i64) -> sqlx::Result<Option<User>> {
    sqlx::query_as("SELECT * FROM users WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await
}

pub async fn user_by_name(pool: &SqlitePool, username: &str) -> sqlx::Result<Option<User>> {
    sqlx::query_as("SELECT * FROM users WHERE username = ?")
        .bind(username)
        .fetch_optional(pool)
        .await
}

pub async fn active_user_by_send_token(pool: &SqlitePool, token: &str) -> sqlx::Result<Option<User>> {
    sqlx::query_as("SELECT * FROM users WHERE send_token = ? AND active = 1")
        .bind(token)
        .fetch_optional(pool)
        .await
}

pub async fn channel_by_key(pool: &SqlitePool, key: &str) -> sqlx::Result<Option<Channel>> {
    sqlx::query_as("SELECT * FROM channels WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await
}

pub async fn has_active_channel(pool: &SqlitePool, user_id: i64) -> sqlx::Result<bool> {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM channels WHERE user_id = ? AND active = 1)")
        .bind(user_id)
        .fetch_one(pool)
        .await
}
