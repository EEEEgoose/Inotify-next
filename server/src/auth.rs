use crate::{
    db::{self, User},
    error::AppError,
    AppState,
};
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use axum::{extract::FromRequestParts, http::request::Parts};
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

const SESSION_HOURS: i64 = 24 * 7;

pub fn hash_password(password: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| anyhow::anyhow!("hash password: {e}"))
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    PasswordHash::new(hash).is_ok_and(|parsed| Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok())
}

#[derive(Clone)]
pub struct JwtKeys {
    encoding: EncodingKey,
    decoding: DecodingKey,
}

#[derive(Serialize, Deserialize)]
struct Claims {
    sub: i64,
    exp: i64,
}

impl JwtKeys {
    pub fn new(secret: &str) -> Self {
        Self {
            encoding: EncodingKey::from_secret(secret.as_bytes()),
            decoding: DecodingKey::from_secret(secret.as_bytes()),
        }
    }

    pub fn issue(&self, user_id: i64) -> anyhow::Result<String> {
        let exp = (chrono::Utc::now() + chrono::Duration::hours(SESSION_HOURS)).timestamp();
        Ok(jsonwebtoken::encode(
            &Header::default(),
            &Claims { sub: user_id, exp },
            &self.encoding,
        )?)
    }

    fn verify(&self, token: &str) -> Option<i64> {
        jsonwebtoken::decode::<Claims>(token, &self.decoding, &Validation::default())
            .ok()
            .map(|data| data.claims.sub)
    }
}

pub fn bearer(parts: &Parts) -> Option<&str> {
    parts
        .headers
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
}

/// A logged-in, active user (from the session JWT).
pub struct AuthUser(pub User);

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, AppError> {
        let user_id = bearer(parts)
            .and_then(|t| state.jwt.verify(t))
            .ok_or(AppError::Unauthorized)?;
        // Re-read the user so disabling or deleting takes effect immediately.
        match db::user_by_id(&state.db, user_id).await? {
            Some(user) if user.active => Ok(Self(user)),
            _ => Err(AppError::Unauthorized),
        }
    }
}

pub struct AdminUser(pub User);

impl FromRequestParts<AppState> for AdminUser {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, AppError> {
        let AuthUser(user) = AuthUser::from_request_parts(parts, state).await?;
        if !user.is_admin {
            return Err(AppError::Forbidden);
        }
        Ok(Self(user))
    }
}
