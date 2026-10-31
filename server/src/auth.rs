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
use tokio::sync::Semaphore;

const SESSION_HOURS: i64 = 24 * 7;

/// Argon2 is deliberately slow and takes 19 MiB per run: run it off the async workers, a few
/// at a time, so a flood of logins cannot stall the server or exhaust memory.
static ARGON2_SLOTS: Semaphore = Semaphore::const_new(4);

pub async fn argon2_blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> anyhow::Result<T> {
    let _permit = ARGON2_SLOTS.acquire().await?;
    Ok(tokio::task::spawn_blocking(f).await?)
}

pub async fn hash_password(password: &str) -> anyhow::Result<String> {
    let password = password.to_owned();
    argon2_blocking(move || {
        let salt = SaltString::generate(&mut OsRng);
        Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map(|h| h.to_string())
            .map_err(|e| anyhow::anyhow!("hash password: {e}"))
    })
    .await?
}

pub async fn verify_password(password: &str, hash: &str) -> anyhow::Result<bool> {
    let (password, hash) = (password.to_owned(), hash.to_owned());
    argon2_blocking(move || {
        PasswordHash::new(&hash)
            .is_ok_and(|parsed| Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok())
    })
    .await
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
    /// The user's `token_version` when issued; tokens from before it was bumped are refused.
    #[serde(default)]
    ver: i64,
}

impl JwtKeys {
    pub fn new(secret: &str) -> Self {
        Self {
            encoding: EncodingKey::from_secret(secret.as_bytes()),
            decoding: DecodingKey::from_secret(secret.as_bytes()),
        }
    }

    pub fn issue(&self, user: &User) -> anyhow::Result<String> {
        let exp = (chrono::Utc::now() + chrono::Duration::hours(SESSION_HOURS)).timestamp();
        let claims = Claims {
            sub: user.id,
            exp,
            ver: user.token_version,
        };
        Ok(jsonwebtoken::encode(&Header::default(), &claims, &self.encoding)?)
    }

    /// Returns (user id, token version).
    fn verify(&self, token: &str) -> Option<(i64, i64)> {
        jsonwebtoken::decode::<Claims>(token, &self.decoding, &Validation::default())
            .ok()
            .map(|data| (data.claims.sub, data.claims.ver))
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
        let (user_id, version) = bearer(parts)
            .and_then(|t| state.jwt.verify(t))
            .ok_or(AppError::Unauthorized)?;
        // Re-read the user so disabling, deleting or a password change takes effect immediately.
        match db::user_by_id(&state.db, user_id).await? {
            Some(user) if user.active && user.token_version == version => Ok(Self(user)),
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
