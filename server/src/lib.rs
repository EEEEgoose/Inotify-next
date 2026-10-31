pub mod api;
pub mod auth;
pub mod channels;
pub mod config;
pub mod db;
pub mod dispatch;
pub mod error;
pub mod net;
pub mod settings;
pub mod throttle;
pub mod turnstile;

use auth::JwtKeys;
use channels::{bark::Apns, SendCtx};
use config::{Config, Endpoints};
use dispatch::Dispatcher;
use settings::Settings;
use sqlx::SqlitePool;
use std::{
    sync::{Arc, RwLock},
    time::Duration,
};
use throttle::FailureLimiter;

#[derive(Clone)]
pub struct AppState {
    pub db: SqlitePool,
    pub jwt: JwtKeys,
    pub dispatcher: Dispatcher,
    pub send_ctx: Arc<SendCtx>,
    pub settings: Arc<RwLock<Settings>>,
    pub endpoints: Arc<Endpoints>,
    pub login_failures: Arc<FailureLimiter>,
    pub trust_proxy: bool,
    pub disable_turnstile: bool,
}

/// Window in which failed logins are counted.
pub const LOGIN_FAILURE_WINDOW: Duration = Duration::from_secs(15 * 60);

impl AppState {
    pub async fn new(db: SqlitePool, config: &Config) -> anyhow::Result<Self> {
        let jwt = JwtKeys::new(&db::jwt_secret(&db).await?);
        let settings = settings::load(&db).await?;
        let send_ctx = Arc::new(SendCtx {
            db: db.clone(),
            net: net::Net::new(config.allow_private_targets, &settings.proxy)?,
            telegram_base: config.endpoints.telegram.clone(),
            wecom_base: config.endpoints.wecom.clone(),
            wecom_tokens: Default::default(),
            apns: Apns::new(config.endpoints.apns.clone())?,
        });
        let dispatcher = Dispatcher::start(send_ctx.clone(), config.send_concurrency);
        Ok(Self {
            db,
            jwt,
            dispatcher,
            send_ctx,
            settings: Arc::new(RwLock::new(settings)),
            endpoints: Arc::new(config.endpoints.clone()),
            login_failures: Arc::new(FailureLimiter::new(LOGIN_FAILURE_WINDOW)),
            trust_proxy: config.trust_proxy,
            disable_turnstile: config.disable_turnstile,
        })
    }

    pub fn settings(&self) -> Settings {
        self.settings.read().unwrap().clone()
    }
}

/// Creates the first administrator with a random password when there are no users yet.
/// Returns the generated password so it can be shown once.
pub async fn bootstrap_admin(db: &SqlitePool) -> anyhow::Result<Option<String>> {
    let users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users").fetch_one(db).await?;
    if users > 0 {
        return Ok(None);
    }
    let password = db::random_token(16);
    db::create_user(db, "admin", Some(&auth::hash_password(&password).await?), true).await?;
    Ok(Some(password))
}
