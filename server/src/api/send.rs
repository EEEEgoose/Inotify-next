use crate::{
    channels::Message,
    db,
    dispatch::Job,
    error::{AppError, AppResult},
    AppState,
};
use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use percent_encoding::percent_decode_str;
use serde::Deserialize;
use serde_json::{json, Map, Value};

/// Resolves a send token: a user's send token targets all their active channels,
/// a channel key targets that one channel.
pub async fn resolve_target(state: &AppState, token: &str) -> AppResult<(i64, Option<String>)> {
    if token.is_empty() {
        return Err(AppError::Unauthorized);
    }
    if let Some(user) = db::active_user_by_send_token(&state.db, token).await? {
        if !db::has_active_channel(&state.db, user.id).await? {
            return Err(AppError::BadRequest("no active channel".into()));
        }
        return Ok((user.id, None));
    }
    let channel = db::channel_by_key(&state.db, token)
        .await?
        .ok_or(AppError::Unauthorized)?;
    if !db::user_by_id(&state.db, channel.user_id)
        .await?
        .is_some_and(|u| u.active)
    {
        return Err(AppError::Unauthorized);
    }
    if !channel.active {
        return Err(AppError::BadRequest("channel is disabled".into()));
    }
    Ok((channel.user_id, Some(channel.key)))
}

async fn enqueue(state: &AppState, token: &str, msg: Message) -> AppResult<()> {
    let (user_id, channel_key) = resolve_target(state, token).await?;
    state.dispatcher.enqueue(Job {
        user_id,
        channel_key,
        msg,
    })
}

#[derive(Deserialize)]
pub struct SendReq {
    title: Option<String>,
    subtitle: Option<String>,
    body: Option<String>,
    /// Extra Bark parameters (url, icon, group, level, ...).
    #[serde(default)]
    extras: Map<String, Value>,
}

/// `POST /api/v1/send` with `Authorization: Bearer <send token or channel key>`.
pub async fn send(State(state): State<AppState>, headers: HeaderMap, Json(req): Json<SendReq>) -> AppResult<Response> {
    let token = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    let msg = Message {
        title: req.title,
        subtitle: req.subtitle,
        body: req.body,
        extras: req.extras,
    };
    enqueue(&state, token, msg).await?;
    Ok((StatusCode::ACCEPTED, Json(json!({ "queued": true }))).into_response())
}

// ---- URL formats of Inotify 2.x, kept so existing scripts only need a new token ----

pub fn compat_routes() -> Router<AppState> {
    Router::new()
        .route("/api/send", get(compat_query))
        .fallback(compat_path)
}

#[derive(Deserialize)]
pub struct CompatQuery {
    token: Option<String>,
    title: Option<String>,
    data: Option<String>,
}

/// `GET /api/send?token=&title=&data=`
async fn compat_query(State(state): State<AppState>, Query(q): Query<CompatQuery>) -> Response {
    compat_send(&state, q.token.as_deref().unwrap_or(""), q.title, q.data).await
}

/// `GET /{token}.send/{title}/{data}`, `/api/{token}.send/...`, and the Bark-style `/?act=x/{token}/{title}/{data}`;
/// anything else is the web UI.
async fn compat_path(State(state): State<AppState>, uri: Uri) -> Response {
    match parse_compat(&uri) {
        Some((token, title, data)) => compat_send(&state, &token, title, data).await,
        None => super::web::serve(&uri),
    }
}

fn decode(s: &str) -> String {
    percent_decode_str(s).decode_utf8_lossy().into_owned()
}

/// Returns (token, title, data). Unencoded slashes after the title stay in `data`.
fn parse_compat(uri: &Uri) -> Option<(String, Option<String>, Option<String>)> {
    let path = uri.path();
    if path == "/" {
        let act = uri.query()?.strip_prefix("act=")?;
        let mut parts = act.splitn(4, '/');
        parts.next()?; // the 2.x Bark server token, no longer needed
        let token = decode(parts.next()?);
        return Some((token, parts.next().map(decode), parts.next().map(decode)));
    }
    let rest = path.strip_prefix("/api/").or_else(|| path.strip_prefix('/'))?;
    let (first, rest) = rest.split_once('/').unwrap_or((rest, ""));
    let token = decode(first.strip_suffix(".send")?);
    let mut parts = rest.splitn(2, '/');
    let title = parts.next().filter(|s| !s.is_empty()).map(decode);
    let data = parts.next().filter(|s| !s.is_empty()).map(decode);
    Some((token, title, data))
}

async fn compat_send(state: &AppState, token: &str, title: Option<String>, data: Option<String>) -> Response {
    let msg = Message {
        title,
        body: data,
        ..Default::default()
    };
    match enqueue(state, token, msg).await {
        Ok(()) => Json(json!({ "code": 200, "message": "success" })).into_response(),
        Err(e) => {
            let status = e.status();
            (
                status,
                Json(json!({ "code": status.as_u16(), "message": e.to_string() })),
            )
                .into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Option<(String, Option<String>, Option<String>)> {
        parse_compat(&s.parse().unwrap())
    }

    #[test]
    fn parses_2x_urls() {
        let full = Some(("TOK".to_string(), Some("标题".to_string()), Some("a/b c".to_string())));
        assert_eq!(parse("/TOK.send/%E6%A0%87%E9%A2%98/a/b%20c"), full);
        assert_eq!(parse("/api/TOK.send/%E6%A0%87%E9%A2%98/a/b%20c"), full);
        assert_eq!(parse("/?act=OLD/TOK/%E6%A0%87%E9%A2%98/a/b%20c"), full);
        assert_eq!(parse("/TOK.send/hi"), Some(("TOK".into(), Some("hi".into()), None)));
        assert_eq!(parse("/TOK.send"), Some(("TOK".into(), None, None)));
        assert_eq!(parse("/?act=OLD/TOK"), Some(("TOK".into(), None, None)));
        assert_eq!(parse("/favicon.ico"), None);
        assert_eq!(parse("/?foo=bar"), None);
    }
}
