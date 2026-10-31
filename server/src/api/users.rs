//! User administration (administrators only).

use super::account::check_password_strength;
use crate::{
    auth::{hash_password, AdminUser},
    db::{self, User},
    error::{AppError, AppResult},
    AppState,
};
use axum::{
    extract::{Path, State},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

pub async fn list(State(state): State<AppState>, _: AdminUser) -> AppResult<Json<Vec<User>>> {
    let users = sqlx::query_as("SELECT * FROM users ORDER BY id")
        .fetch_all(&state.db)
        .await?;
    Ok(Json(users))
}

#[derive(Deserialize)]
pub struct CreateReq {
    username: String,
    password: String,
    #[serde(default)]
    is_admin: bool,
}

pub async fn create(State(state): State<AppState>, _: AdminUser, Json(req): Json<CreateReq>) -> AppResult<Json<User>> {
    let username = req.username.trim();
    if username.is_empty() {
        return Err(AppError::BadRequest("用户名不能为空".into()));
    }
    check_password_strength(&req.password)?;
    if db::user_by_name(&state.db, username).await?.is_some() {
        return Err(AppError::Conflict("用户名已存在".into()));
    }
    Ok(Json(
        db::create_user(&state.db, username, Some(&hash_password(&req.password)?), req.is_admin).await?,
    ))
}

#[derive(Deserialize)]
pub struct UpdateReq {
    active: Option<bool>,
    is_admin: Option<bool>,
    password: Option<String>,
}

pub async fn update(
    State(state): State<AppState>,
    AdminUser(admin): AdminUser,
    Path(id): Path<i64>,
    Json(req): Json<UpdateReq>,
) -> AppResult<Json<User>> {
    let user = db::user_by_id(&state.db, id).await?.ok_or(AppError::NotFound)?;
    if id == admin.id && (req.active == Some(false) || req.is_admin == Some(false)) {
        return Err(AppError::BadRequest("不能禁用自己或取消自己的管理员权限".into()));
    }
    let password_hash = match &req.password {
        Some(p) => {
            check_password_strength(p)?;
            Some(hash_password(p)?)
        }
        None => user.password_hash,
    };
    let user = sqlx::query_as("UPDATE users SET active = ?, is_admin = ?, password_hash = ? WHERE id = ? RETURNING *")
        .bind(req.active.unwrap_or(user.active))
        .bind(req.is_admin.unwrap_or(user.is_admin))
        .bind(password_hash)
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    Ok(Json(user))
}

pub async fn remove(
    State(state): State<AppState>,
    AdminUser(admin): AdminUser,
    Path(id): Path<i64>,
) -> AppResult<Json<Value>> {
    if id == admin.id {
        return Err(AppError::BadRequest("不能删除自己".into()));
    }
    // Channels go with the user (ON DELETE CASCADE).
    let deleted = sqlx::query("DELETE FROM users WHERE id = ?")
        .bind(id)
        .execute(&state.db)
        .await?;
    if deleted.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(Json(json!({})))
}
