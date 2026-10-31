//! Password-encrypted export and import of a user's own channels.
//!
//! The file is JSON: the channel list encrypted with AES-256-GCM, keyed by Argon2id(password, salt).

use crate::{
    auth::AuthUser,
    channels::Kind,
    db::{self, Channel},
    error::{AppError, AppResult},
    AppState,
};
use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit};
use anyhow::anyhow;
use argon2::{Algorithm, Argon2, Params, Version};
use axum::{extract::State, Json};
use base64::{engine::general_purpose::STANDARD, Engine};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const FORMAT: &str = "inotify-next-channels";
const VERSION: u32 = 1;
const AAD: &[u8] = b"inotify-next-channels/1";

#[derive(Serialize, Deserialize)]
pub struct BackupFile {
    format: String,
    version: u32,
    salt: String,
    nonce: String,
    data: String,
}

#[derive(Serialize, Deserialize)]
struct Entry {
    kind: String,
    name: String,
    key: String,
    config: Value,
    active: bool,
}

/// Argon2id with the crate's default (OWASP) cost; part of the version 1 format.
async fn derive_key(password: String, salt: Vec<u8>) -> AppResult<[u8; 32]> {
    tokio::task::spawn_blocking(move || {
        let mut key = [0u8; 32];
        Argon2::new(Algorithm::Argon2id, Version::V0x13, Params::default())
            .hash_password_into(password.as_bytes(), &salt, &mut key)
            .map_err(|e| anyhow!("argon2: {e}"))?;
        Ok(key)
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))?
}

#[derive(Deserialize)]
pub struct ExportReq {
    password: String,
}

pub async fn export(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Json(req): Json<ExportReq>,
) -> AppResult<Json<BackupFile>> {
    if req.password.chars().count() < 8 {
        return Err(AppError::BadRequest("备份密码至少 8 位".into()));
    }
    let channels: Vec<Channel> = sqlx::query_as("SELECT * FROM channels WHERE user_id = ? ORDER BY id")
        .bind(user.id)
        .fetch_all(&state.db)
        .await?;
    let entries: Vec<Entry> = channels
        .into_iter()
        .map(|c| Entry {
            config: serde_json::from_str(&c.config).unwrap_or_default(),
            kind: c.kind,
            name: c.name,
            key: c.key,
            active: c.active,
        })
        .collect();
    let plaintext = serde_json::to_vec(&json!({ "channels": entries })).map_err(|e| AppError::Internal(e.into()))?;

    let mut salt = [0u8; 16];
    let mut nonce = [0u8; 12];
    rand::rngs::OsRng.fill_bytes(&mut salt);
    rand::rngs::OsRng.fill_bytes(&mut nonce);
    let key = derive_key(req.password, salt.to_vec()).await?;
    let data = Aes256Gcm::new(&key.into())
        .encrypt(
            &nonce.into(),
            aes_gcm::aead::Payload {
                msg: &plaintext,
                aad: AAD,
            },
        )
        .map_err(|_| anyhow!("encryption failed"))?;
    Ok(Json(BackupFile {
        format: FORMAT.into(),
        version: VERSION,
        salt: STANDARD.encode(salt),
        nonce: STANDARD.encode(nonce),
        data: STANDARD.encode(data),
    }))
}

#[derive(Deserialize)]
pub struct ImportReq {
    password: String,
    backup: BackupFile,
}

fn decode(field: &str, value: &str) -> AppResult<Vec<u8>> {
    STANDARD
        .decode(value)
        .map_err(|_| AppError::BadRequest(format!("备份文件损坏（{field}）")))
}

/// Keys from the file are kept when free; a channel whose key the user already has is skipped,
/// and a key used by someone else (or malformed) is replaced with a new one.
pub async fn import(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Json(req): Json<ImportReq>,
) -> AppResult<Json<Value>> {
    let backup = req.backup;
    if backup.format != FORMAT || backup.version != VERSION {
        return Err(AppError::BadRequest("不是 Inotify-next 渠道备份文件".into()));
    }
    let salt = decode("salt", &backup.salt)?;
    let nonce = decode("nonce", &backup.nonce)?;
    let data = decode("data", &backup.data)?;
    if salt.len() < 8 || nonce.len() != 12 {
        return Err(AppError::BadRequest("备份文件损坏".into()));
    }
    let key = derive_key(req.password, salt).await?;
    let plaintext = Aes256Gcm::new(&key.into())
        .decrypt(nonce.as_slice().into(), aes_gcm::aead::Payload { msg: &data, aad: AAD })
        .map_err(|_| AppError::BadRequest("密码错误或备份文件已损坏".into()))?;
    #[derive(Deserialize)]
    struct Contents {
        channels: Vec<Entry>,
    }
    let contents: Contents =
        serde_json::from_slice(&plaintext).map_err(|_| AppError::BadRequest("备份内容格式错误".into()))?;

    // Validate everything before writing anything.
    let mut prepared = Vec::new();
    for entry in contents.channels {
        let kind =
            Kind::parse(&entry.kind).ok_or_else(|| AppError::BadRequest(format!("未知渠道类型 {}", entry.kind)))?;
        // Readonly fields (the Bark device token) come from the backup itself.
        let config = kind
            .normalize_config(&entry.config, Some(&entry.config))
            .map_err(|e| AppError::BadRequest(format!("渠道「{}」：{e}", entry.name)))?;
        prepared.push((kind, entry, config));
    }

    let (mut imported, mut skipped, mut rekeyed) = (0, 0, 0);
    let mut tx = state.db.begin().await?;
    for (kind, entry, config) in prepared {
        let well_formed = (16..=64).contains(&entry.key.len()) && entry.key.chars().all(|c| c.is_ascii_alphanumeric());
        let owner: Option<i64> = sqlx::query_scalar("SELECT user_id FROM channels WHERE key = ?")
            .bind(&entry.key)
            .fetch_optional(&mut *tx)
            .await?;
        if owner == Some(user.id) {
            skipped += 1;
            continue;
        }
        let is_send_token: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE send_token = ?)")
            .bind(&entry.key)
            .fetch_one(&mut *tx)
            .await?;
        let key = if well_formed && owner.is_none() && !is_send_token {
            entry.key
        } else {
            rekeyed += 1;
            db::random_token(32)
        };
        sqlx::query("INSERT INTO channels (user_id, kind, name, key, config, active) VALUES (?, ?, ?, ?, ?, ?)")
            .bind(user.id)
            .bind(kind.as_str())
            .bind(super::channels::non_empty_name(&entry.name, kind))
            .bind(key)
            .bind(config.to_string())
            .bind(entry.active)
            .execute(&mut *tx)
            .await?;
        imported += 1;
    }
    tx.commit().await?;
    Ok(Json(
        json!({ "imported": imported, "skipped": skipped, "rekeyed": rekeyed }),
    ))
}
