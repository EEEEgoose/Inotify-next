use crate::{
    auth::AuthUser,
    channels::{Kind, KindInfo, Message},
    db::{self, Channel},
    dispatch,
    error::{AppError, AppResult},
    AppState,
};
use axum::{
    extract::{Path, State},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

pub async fn kinds(_: AuthUser) -> Json<Vec<KindInfo>> {
    Json(Kind::ALL.into_iter().map(Kind::info).collect())
}

pub async fn list(State(state): State<AppState>, AuthUser(user): AuthUser) -> AppResult<Json<Vec<Channel>>> {
    let channels = sqlx::query_as("SELECT * FROM channels WHERE user_id = ? ORDER BY id")
        .bind(user.id)
        .fetch_all(&state.db)
        .await?;
    Ok(Json(channels))
}

async fn owned(state: &AppState, user_id: i64, id: i64) -> AppResult<Channel> {
    sqlx::query_as("SELECT * FROM channels WHERE id = ? AND user_id = ?")
        .bind(id)
        .bind(user_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)
}

#[derive(Deserialize)]
pub struct CreateReq {
    kind: String,
    name: String,
    config: Value,
    #[serde(default = "yes")]
    active: bool,
}

fn yes() -> bool {
    true
}

pub async fn create(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Json(req): Json<CreateReq>,
) -> AppResult<Json<Channel>> {
    let kind = Kind::parse(&req.kind).ok_or_else(|| AppError::BadRequest(format!("unknown kind {}", req.kind)))?;
    if !kind.info().user_creatable {
        return Err(AppError::BadRequest(format!(
            "{} 渠道由 App 注册自动创建",
            kind.info().name
        )));
    }
    let config = kind.normalize_config(&req.config, None).map_err(AppError::BadRequest)?;
    let channel = sqlx::query_as(
        "INSERT INTO channels (user_id, kind, name, key, config, active) VALUES (?, ?, ?, ?, ?, ?) RETURNING *",
    )
    .bind(user.id)
    .bind(kind.as_str())
    .bind(non_empty_name(&req.name, kind))
    .bind(db::random_token(32))
    .bind(config.to_string())
    .bind(req.active)
    .fetch_one(&state.db)
    .await?;
    Ok(Json(channel))
}

pub(super) fn non_empty_name(name: &str, kind: Kind) -> String {
    match name.trim() {
        "" => kind.info().name.to_string(),
        name => name.to_string(),
    }
}

#[derive(Deserialize)]
pub struct UpdateReq {
    name: Option<String>,
    config: Option<Value>,
    active: Option<bool>,
}

pub async fn update(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<i64>,
    Json(req): Json<UpdateReq>,
) -> AppResult<Json<Channel>> {
    let channel = owned(&state, user.id, id).await?;
    let kind = Kind::parse(&channel.kind).ok_or_else(|| AppError::BadRequest("unknown kind".into()))?;
    let config = match &req.config {
        Some(input) => {
            let previous: Value = serde_json::from_str(&channel.config).unwrap_or_default();
            kind.normalize_config(input, Some(&previous))
                .map_err(AppError::BadRequest)?
                .to_string()
        }
        None => channel.config,
    };
    let channel = sqlx::query_as(
        "UPDATE channels SET name = ?, config = ?, active = ?, updated_at = datetime('now') WHERE id = ? RETURNING *",
    )
    .bind(req.name.map(|n| non_empty_name(&n, kind)).unwrap_or(channel.name))
    .bind(config)
    .bind(req.active.unwrap_or(channel.active))
    .bind(id)
    .fetch_one(&state.db)
    .await?;
    Ok(Json(channel))
}

pub async fn remove(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<i64>,
) -> AppResult<Json<Value>> {
    owned(&state, user.id, id).await?;
    sqlx::query("DELETE FROM channels WHERE id = ?")
        .bind(id)
        .execute(&state.db)
        .await?;
    Ok(Json(json!({})))
}

/// Sends a test message right away and reports the channel's own error, if any.
pub async fn test(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<i64>,
) -> AppResult<Json<Value>> {
    let channel = owned(&state, user.id, id).await?;
    let msg = Message {
        title: Some("Inotify 测试消息".into()),
        body: Some(format!("渠道「{}」配置正确", channel.name)),
        ..Default::default()
    };
    match dispatch::send_one(&state.send_ctx, &channel, &msg).await {
        Ok(()) => Ok(Json(json!({ "ok": true }))),
        Err(e) => Ok(Json(json!({ "ok": false, "error": format!("{e:#}") }))),
    }
}
