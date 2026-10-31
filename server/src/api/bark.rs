//! Bark-compatible server under `/bark/{send_token}`.
//!
//! Users enter `https://<host>/bark/<send token>` as the server address in the official Bark app.
//! The app registers its device token here, which creates (or refreshes) a Bark channel for that
//! user. Tools that speak the Bark push API can also push through this prefix: the key is either a
//! channel key (that channel only) or the user's send token (all active channels).
//! Request and response shapes follow github.com/Finb/bark-server.

use crate::{
    channels::{bark::initial_config, Kind, Message},
    db::{self, User},
    dispatch::Job,
    error::AppError,
    AppState,
};
use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{header::CONTENT_TYPE, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde_json::{json, Map, Value};
use std::collections::HashMap;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/bark/{token}/ping", get(ping))
        .route("/bark/{token}/healthz", get(|| async { "ok" }))
        .route("/bark/{token}/info", get(info))
        .route("/bark/{token}/register", get(register).post(register))
        .route("/bark/{token}/register/{device_key}", get(register_check))
        .route("/bark/{token}/push", axum::routing::post(push))
        .route("/bark/{token}/{key}", get(push).post(push))
        .route("/bark/{token}/{key}/{body}", get(push).post(push))
        .route("/bark/{token}/{key}/{title}/{body}", get(push).post(push))
        .route("/bark/{token}/{key}/{title}/{subtitle}/{body}", get(push).post(push))
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

fn ok(message: &str) -> Response {
    Json(json!({ "code": 200, "message": message, "timestamp": now() })).into_response()
}

fn data(value: Value) -> Response {
    Json(json!({ "code": 200, "message": "success", "data": value, "timestamp": now() })).into_response()
}

fn failed(status: StatusCode, message: impl Into<String>) -> Response {
    let body = json!({ "code": status.as_u16(), "message": message.into(), "timestamp": now() });
    (status, Json(body)).into_response()
}

fn internal(e: impl Into<AppError>) -> Response {
    let e: AppError = e.into();
    failed(e.status(), e.to_string())
}

async fn user_for(state: &AppState, token: &str) -> Result<User, Response> {
    match db::active_user_by_send_token(&state.db, token).await {
        Ok(Some(user)) => Ok(user),
        Ok(None) => Err(failed(
            StatusCode::BAD_REQUEST,
            "invalid server address: unknown send token",
        )),
        Err(e) => Err(internal(e)),
    }
}

/// Query parameters, then the body (JSON or form), then path parameters; later sources win.
fn collect_params(
    query: HashMap<String, String>,
    headers: &HeaderMap,
    body: &[u8],
    path: HashMap<String, String>,
) -> Result<Map<String, Value>, String> {
    let mut params = Map::new();
    let is_json = headers
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("application/json") || v.contains("+json"));
    if is_json && !body.is_empty() {
        match serde_json::from_slice::<Value>(body) {
            Ok(Value::Object(map)) => params.extend(map),
            _ => return Err("request body must be a JSON object".into()),
        }
    } else if !body.is_empty() {
        let form: Vec<(String, String)> = serde_urlencoded::from_bytes(body).map_err(|e| e.to_string())?;
        params.extend(form.into_iter().map(|(k, v)| (k.to_lowercase(), Value::String(v))));
    }
    // bark-server lets query parameters override the JSON body too.
    params.extend(query.into_iter().map(|(k, v)| (k.to_lowercase(), Value::String(v))));
    params.extend(path.into_iter().map(|(k, v)| (k, Value::String(v))));
    Ok(params)
}

fn take_string(params: &mut Map<String, Value>, key: &str) -> Option<String> {
    match params.remove(key)? {
        Value::String(s) => Some(s),
        Value::Null => None,
        other => Some(other.to_string()),
    }
}

async fn ping(State(state): State<AppState>, Path(token): Path<String>) -> Response {
    match user_for(&state, &token).await {
        Ok(_) => ok("pong"),
        Err(resp) => resp,
    }
}

async fn info(State(state): State<AppState>, Path(token): Path<String>) -> Response {
    let user = match user_for(&state, &token).await {
        Ok(user) => user,
        Err(resp) => return resp,
    };
    let devices: Result<i64, _> =
        sqlx::query_scalar("SELECT COUNT(*) FROM channels WHERE user_id = ? AND kind = 'bark'")
            .bind(user.id)
            .fetch_one(&state.db)
            .await;
    match devices {
        Ok(devices) => Json(json!({
            "version": env!("CARGO_PKG_VERSION"),
            "build": "",
            "arch": format!("{}/{}", std::env::consts::OS, std::env::consts::ARCH),
            "commit": "",
            "devices": devices,
        }))
        .into_response(),
        Err(e) => internal(e),
    }
}

async fn register(
    State(state): State<AppState>,
    Path(token): Path<String>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let user = match user_for(&state, &token).await {
        Ok(user) => user,
        Err(resp) => return resp,
    };
    let mut params = match collect_params(query, &headers, &body, HashMap::new()) {
        Ok(p) => p,
        Err(e) => return failed(StatusCode::BAD_REQUEST, format!("request bind failed: {e}")),
    };
    // `key` and `devicetoken` are the names older app versions use.
    let device_key = take_string(&mut params, "device_key")
        .or_else(|| take_string(&mut params, "key"))
        .unwrap_or_default();
    let device_token = take_string(&mut params, "device_token")
        .or_else(|| take_string(&mut params, "devicetoken"))
        .unwrap_or_default();
    if device_token.is_empty() {
        return failed(StatusCode::BAD_REQUEST, "device token is empty");
    }
    if device_token.len() > 160 {
        return failed(StatusCode::BAD_REQUEST, "device token is invalid");
    }

    match register_device(&state, &user, &device_key, &device_token).await {
        Ok(key) => data(json!({ "key": key, "device_key": key, "device_token": device_token })),
        Err(e) => failed(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("device registration failed: {e}"),
        ),
    }
}

/// Refreshes the token of the user's existing Bark channel with this key, or creates a channel.
/// A key owned by someone else (or by a non-Bark channel) is never reused.
async fn register_device(state: &AppState, user: &User, device_key: &str, device_token: &str) -> sqlx::Result<String> {
    let existing = match device_key {
        "" => None,
        key => db::channel_by_key(&state.db, key).await?,
    };
    if let Some(channel) = &existing {
        if channel.user_id == user.id && channel.kind == Kind::Bark.as_str() {
            sqlx::query("UPDATE channels SET config = json_set(config, '$.device_token', ?), updated_at = datetime('now') WHERE id = ?")
                .bind(device_token)
                .bind(channel.id)
                .execute(&state.db)
                .await?;
            return Ok(channel.key.clone());
        }
    }
    let key = if device_key.is_empty() || existing.is_some() {
        db::random_token(22)
    } else {
        device_key.to_string()
    };
    sqlx::query("INSERT INTO channels (user_id, kind, name, key, config) VALUES (?, ?, ?, ?, ?)")
        .bind(user.id)
        .bind(Kind::Bark.as_str())
        .bind("Bark")
        .bind(&key)
        .bind(initial_config(device_token).to_string())
        .execute(&state.db)
        .await?;
    Ok(key)
}

async fn register_check(State(state): State<AppState>, Path((token, device_key)): Path<(String, String)>) -> Response {
    let user = match user_for(&state, &token).await {
        Ok(user) => user,
        Err(resp) => return resp,
    };
    match db::channel_by_key(&state.db, &device_key).await {
        Ok(Some(c)) if c.user_id == user.id && c.kind == Kind::Bark.as_str() => ok("success"),
        Ok(_) => failed(StatusCode::BAD_REQUEST, "device not registered"),
        Err(e) => internal(e),
    }
}

async fn push(
    State(state): State<AppState>,
    Path(mut path): Path<HashMap<String, String>>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let token = path.remove("token").unwrap_or_default();
    let user = match user_for(&state, &token).await {
        Ok(user) => user,
        Err(resp) => return resp,
    };
    if let Some(key) = path.remove("key") {
        path.insert("device_key".into(), key);
    }
    let mut params = match collect_params(query, &headers, &body, path) {
        Ok(p) => p,
        Err(e) => return failed(StatusCode::BAD_REQUEST, format!("request bind failed: {e}")),
    };

    let batch: Vec<String> = match params.remove("device_keys") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::String(s)) => s
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect(),
        Some(Value::Array(items)) => items.iter().filter_map(Value::as_str).map(String::from).collect(),
        Some(_) => return failed(StatusCode::BAD_REQUEST, "invalid type for device_keys"),
    };
    let device_key = take_string(&mut params, "device_key").unwrap_or_default();
    let msg = Message {
        title: take_string(&mut params, "title"),
        subtitle: take_string(&mut params, "subtitle"),
        body: take_string(&mut params, "body"),
        extras: params,
    };

    if batch.is_empty() {
        return match push_one(&state, &user, &device_key, msg).await {
            Ok(()) => ok("success"),
            Err((status, message)) => failed(status, message),
        };
    }
    let mut results = Vec::with_capacity(batch.len());
    for key in batch {
        let result = match push_one(&state, &user, &key, msg.clone()).await {
            Ok(()) => json!({ "code": 200, "device_key": key }),
            Err((status, message)) => json!({ "code": status.as_u16(), "device_key": key, "message": message }),
        };
        results.push(result);
    }
    data(Value::Array(results))
}

async fn push_one(state: &AppState, user: &User, key: &str, msg: Message) -> Result<(), (StatusCode, String)> {
    if key.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "device key is empty".into()));
    }
    let channel_key = if key == user.send_token {
        None
    } else {
        match db::channel_by_key(&state.db, key).await {
            Ok(Some(c)) if c.user_id == user.id && c.active => Some(c.key),
            Ok(Some(c)) if c.user_id == user.id => return Err((StatusCode::BAD_REQUEST, "channel is disabled".into())),
            Ok(_) => {
                return Err((
                    StatusCode::BAD_REQUEST,
                    "failed to get device token: device not registered".into(),
                ))
            }
            Err(e) => return Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
        }
    };
    state
        .dispatcher
        .enqueue(Job {
            user_id: user.id,
            channel_key,
            msg,
        })
        .map_err(|e| (e.status(), e.to_string()))
}
