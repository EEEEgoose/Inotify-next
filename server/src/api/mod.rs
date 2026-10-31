mod account;
mod admin;
mod backup;
mod bark;
mod channels;
mod send;
mod users;
mod web;

use crate::AppState;
use axum::{
    extract::Request,
    http::{header, HeaderValue},
    middleware::{self, Next},
    response::Response,
    routing::{get, post, put},
    Router,
};

/// Scripts and frames only from this origin and Cloudflare Turnstile; images also from `data:`
/// (QR codes). Element Plus sets inline styles.
const CSP: &str = "default-src 'self'; script-src 'self' https://challenges.cloudflare.com; \
    frame-src https://challenges.cloudflare.com; style-src 'self' 'unsafe-inline'; \
    img-src 'self' data:; connect-src 'self'; \
    object-src 'none'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'";

async fn security_headers(req: Request, next: Next) -> Response {
    let mut resp = next.run(req).await;
    let headers = resp.headers_mut();
    headers.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static(CSP));
    headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    headers.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("strict-origin-when-cross-origin"),
    );
    resp
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/login", post(account::login))
        .route("/api/v1/me", get(account::me))
        .route("/api/v1/me/password", put(account::change_password))
        .route("/api/v1/me/send-token", post(account::reset_send_token))
        .route("/api/v1/auth-options", get(account::auth_options))
        .route("/api/v1/settings", get(admin::get_settings).put(admin::put_settings))
        .route("/api/v1/stats", get(admin::stats))
        .route("/api/v1/channel-kinds", get(channels::kinds))
        .route("/api/v1/channels", get(channels::list).post(channels::create))
        .route("/api/v1/channels/export", post(backup::export))
        .route("/api/v1/channels/import", post(backup::import))
        .route("/api/v1/channels/{id}", put(channels::update).delete(channels::remove))
        .route("/api/v1/channels/{id}/test", post(channels::test))
        .route("/api/v1/users", get(users::list).post(users::create))
        .route("/api/v1/users/{id}", put(users::update).delete(users::remove))
        .route("/api/v1/send", post(send::send))
        .merge(bark::routes())
        .merge(send::compat_routes())
        .layer(middleware::from_fn(security_headers))
        .with_state(state)
}
