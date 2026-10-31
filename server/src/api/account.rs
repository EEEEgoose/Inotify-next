use crate::{
    auth::{hash_password, verify_password, AuthUser},
    db::{self, User},
    error::{AppError, AppResult},
    throttle::ClientIp,
    turnstile, AppState,
};
use axum::{extract::State, Json};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::OnceCell;

/// Failed logins allowed per client IP, and per username (from any number of IPs), in
/// `LOGIN_FAILURE_WINDOW`. The per-user limit is higher so one client cannot lock an account out.
const MAX_FAILURES_PER_IP: u32 = 10;
const MAX_FAILURES_PER_USER: u32 = 20;

pub const MIN_PASSWORD_LEN: usize = 8;

pub fn check_password_strength(password: &str) -> AppResult<()> {
    if password.chars().count() < MIN_PASSWORD_LEN {
        return Err(AppError::BadRequest(format!("密码至少 {MIN_PASSWORD_LEN} 位")));
    }
    Ok(())
}

#[derive(Deserialize)]
pub struct LoginReq {
    username: String,
    password: String,
    turnstile_token: Option<String>,
}

/// What the login page needs: the Turnstile site key when logins must pass it.
pub async fn auth_options(State(state): State<AppState>) -> Json<Value> {
    Json(json!({ "turnstile_site_key": turnstile::site_key(&state) }))
}

/// Checked against when the user does not exist, so the response time does not reveal that.
async fn dummy_hash() -> AppResult<&'static str> {
    static HASH: OnceCell<String> = OnceCell::const_new();
    Ok(HASH.get_or_try_init(|| hash_password("inotify-dummy-password")).await?)
}

pub async fn login(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    Json(req): Json<LoginReq>,
) -> AppResult<Json<Value>> {
    let ip_key = format!("ip:{}", ip.map_or("unknown".into(), |ip| ip.to_string()));
    let user_key = format!("user:{}", req.username.chars().take(64).collect::<String>());
    let limiter = &state.login_failures;
    if limiter.blocked(&ip_key, MAX_FAILURES_PER_IP) || limiter.blocked(&user_key, MAX_FAILURES_PER_USER) {
        return Err(AppError::TooManyRequests("登录失败次数过多，请 15 分钟后再试".into()));
    }
    turnstile::check(&state, req.turnstile_token.as_deref(), ip).await?;

    let user = db::user_by_name(&state.db, &req.username).await?;
    let hash = user
        .as_ref()
        .filter(|u| u.active)
        .and_then(|u| u.password_hash.as_deref());
    let valid = match hash {
        Some(hash) => verify_password(&req.password, hash).await?,
        None => {
            verify_password(&req.password, dummy_hash().await?).await?;
            false
        }
    };
    match user {
        Some(user) if valid => {
            limiter.clear(&user_key);
            Ok(Json(json!({ "token": state.jwt.issue(&user)?, "user": user })))
        }
        _ => {
            limiter.fail(&ip_key);
            limiter.fail(&user_key);
            Err(AppError::BadRequest("用户名或密码错误".into()))
        }
    }
}

pub async fn me(AuthUser(user): AuthUser) -> Json<User> {
    Json(user)
}

#[derive(Deserialize)]
pub struct ChangePasswordReq {
    #[serde(default)]
    old_password: String,
    new_password: String,
}

/// Changes the password (an account without one sets it without an old password).
/// Every other session ends; this one continues with the returned token.
pub async fn change_password(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Json(req): Json<ChangePasswordReq>,
) -> AppResult<Json<Value>> {
    if let Some(hash) = &user.password_hash {
        if !verify_password(&req.old_password, hash).await? {
            return Err(AppError::BadRequest("原密码错误".into()));
        }
    }
    check_password_strength(&req.new_password)?;
    let user: User = sqlx::query_as(
        "UPDATE users SET password_hash = ?, token_version = token_version + 1 WHERE id = ? RETURNING *",
    )
    .bind(hash_password(&req.new_password).await?)
    .bind(user.id)
    .fetch_one(&state.db)
    .await?;
    Ok(Json(json!({ "token": state.jwt.issue(&user)? })))
}

/// Issues a new send token; the old one stops working at once.
pub async fn reset_send_token(State(state): State<AppState>, AuthUser(user): AuthUser) -> AppResult<Json<User>> {
    let user = sqlx::query_as("UPDATE users SET send_token = ? WHERE id = ? RETURNING *")
        .bind(db::random_token(32))
        .bind(user.id)
        .fetch_one(&state.db)
        .await?;
    Ok(Json(user))
}
