//! Cloudflare Turnstile for the login form: the widget gives the browser a single-use token,
//! which the server checks with Cloudflare before looking at the password.

use crate::{
    error::{AppError, AppResult},
    AppState,
};
use serde_json::Value;
use std::net::IpAddr;

/// Cloudflare's limit on the token length.
const MAX_TOKEN_LEN: usize = 2048;

/// The site key for the login page, when logins must pass Turnstile.
pub fn site_key(state: &AppState) -> Option<String> {
    let settings = state.settings();
    (!state.disable_turnstile && settings.turnstile_ready()).then_some(settings.turnstile_site_key)
}

pub async fn check(state: &AppState, token: Option<&str>, ip: Option<IpAddr>) -> AppResult<()> {
    let settings = state.settings();
    if state.disable_turnstile || !settings.turnstile_ready() {
        return Ok(());
    }
    let token = token
        .filter(|t| !t.is_empty() && t.len() <= MAX_TOKEN_LEN)
        .ok_or_else(|| AppError::BadRequest("请完成人机验证".into()))?;
    let ip = ip.map(|ip| ip.to_string());
    let mut form = vec![("secret", settings.turnstile_secret_key.as_str()), ("response", token)];
    if let Some(ip) = &ip {
        form.push(("remoteip", ip));
    }
    let result: anyhow::Result<Value> = async {
        Ok(state
            .send_ctx
            .net
            .client()
            .post(format!("{}/turnstile/v0/siteverify", state.endpoints.turnstile))
            .form(&form)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }
    .await;
    match result {
        Ok(v) if v["success"] == true => Ok(()),
        Ok(v) => {
            tracing::info!("Turnstile rejected a login: {}", v["error-codes"]);
            Err(AppError::BadRequest("人机验证失败，请重试".into()))
        }
        Err(e) => {
            tracing::warn!("Turnstile siteverify: {e:#}");
            Err(AppError::BadRequest("人机验证服务暂时不可用，请稍后再试".into()))
        }
    }
}
