mod account;
mod admin;
mod bark;
mod channels;
mod oauth;
mod send;
mod users;
mod web;

use crate::AppState;
use axum::{
    routing::{delete, get, post, put},
    Router,
};

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/login", post(account::login))
        .route("/api/v1/me", get(account::me))
        .route("/api/v1/me/password", put(account::change_password))
        .route("/api/v1/me/send-token", post(account::reset_send_token))
        .route("/api/v1/me/github", delete(account::unlink_github))
        .route("/api/v1/auth-options", get(oauth::auth_options))
        .route("/api/v1/oauth/github/start", get(oauth::start))
        .route("/api/v1/oauth/github/link", post(oauth::link))
        .route("/api/v1/oauth/github/callback", get(oauth::callback))
        .route("/api/v1/settings", get(admin::get_settings).put(admin::put_settings))
        .route("/api/v1/stats", get(admin::stats))
        .route("/api/v1/channel-kinds", get(channels::kinds))
        .route("/api/v1/channels", get(channels::list).post(channels::create))
        .route("/api/v1/channels/{id}", put(channels::update).delete(channels::remove))
        .route("/api/v1/channels/{id}/test", post(channels::test))
        .route("/api/v1/users", get(users::list).post(users::create))
        .route("/api/v1/users/{id}", put(users::update).delete(users::remove))
        .route("/api/v1/send", post(send::send))
        .merge(bark::routes())
        .merge(send::compat_routes())
        .with_state(state)
}
