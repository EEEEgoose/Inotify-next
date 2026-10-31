use crate::{
    auth::{hash_password, verify_password, AuthUser},
    db::{self, User},
    error::{AppError, AppResult},
    AppState,
};
use axum::{extract::State, Json};
use serde::Deserialize;
use serde_json::{json, Value};

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
}

pub async fn login(State(state): State<AppState>, Json(req): Json<LoginReq>) -> AppResult<Json<Value>> {
    let user = db::user_by_name(&state.db, &req.username).await?;
    let valid = user.as_ref().is_some_and(|u| {
        u.active
            && u.password_hash
                .as_deref()
                .is_some_and(|h| verify_password(&req.password, h))
    });
    match user {
        Some(user) if valid => Ok(Json(json!({ "token": state.jwt.issue(user.id)?, "user": user }))),
        _ => Err(AppError::BadRequest("用户名或密码错误".into())),
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

/// Changes the password; accounts created through GitHub set their first one without an old password.
pub async fn change_password(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Json(req): Json<ChangePasswordReq>,
) -> AppResult<Json<Value>> {
    if let Some(hash) = &user.password_hash {
        if !verify_password(&req.old_password, hash) {
            return Err(AppError::BadRequest("原密码错误".into()));
        }
    }
    check_password_strength(&req.new_password)?;
    sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?")
        .bind(hash_password(&req.new_password)?)
        .bind(user.id)
        .execute(&state.db)
        .await?;
    Ok(Json(json!({})))
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

/// Unlinks GitHub; refused when it is the account's only way to log in.
pub async fn unlink_github(State(state): State<AppState>, AuthUser(user): AuthUser) -> AppResult<Json<User>> {
    if !user.has_password() {
        return Err(AppError::BadRequest("请先设置密码再解绑 GitHub".into()));
    }
    let user = sqlx::query_as("UPDATE users SET github_id = NULL, github_login = NULL WHERE id = ? RETURNING *")
        .bind(user.id)
        .fetch_one(&state.db)
        .await?;
    Ok(Json(user))
}
