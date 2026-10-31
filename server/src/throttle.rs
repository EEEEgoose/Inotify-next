//! Login throttling: failed attempts are counted per client IP and per username in a fixed window.

use crate::AppState;
use axum::{
    extract::{ConnectInfo, FromRequestParts},
    http::request::Parts,
};
use std::{
    collections::HashMap,
    convert::Infallible,
    net::{IpAddr, SocketAddr},
    sync::Mutex,
    time::{Duration, Instant},
};

/// Bounds memory when someone tries many distinct usernames or addresses.
const MAX_KEYS: usize = 100_000;

pub struct FailureLimiter {
    window: Duration,
    entries: Mutex<HashMap<String, (u32, Instant)>>,
}

impl FailureLimiter {
    pub fn new(window: Duration) -> Self {
        Self {
            window,
            entries: Default::default(),
        }
    }

    /// True when `key` already has `max` failures in the current window.
    pub fn blocked(&self, key: &str, max: u32) -> bool {
        self.entries
            .lock()
            .unwrap()
            .get(key)
            .is_some_and(|(n, since)| since.elapsed() < self.window && *n >= max)
    }

    pub fn fail(&self, key: &str) {
        let mut entries = self.entries.lock().unwrap();
        if entries.len() >= MAX_KEYS {
            entries.retain(|_, (_, since)| since.elapsed() < self.window);
            if entries.len() >= MAX_KEYS && !entries.contains_key(key) {
                return;
            }
        }
        let entry = entries.entry(key.to_string()).or_insert((0, Instant::now()));
        if entry.1.elapsed() >= self.window {
            *entry = (0, Instant::now());
        }
        entry.0 += 1;
    }

    pub fn clear(&self, key: &str) {
        self.entries.lock().unwrap().remove(key);
    }
}

/// The client's address: the last `X-Forwarded-For` hop when the proxy is trusted, otherwise the
/// peer address. `None` when neither is known (e.g. in tests).
pub struct ClientIp(pub Option<IpAddr>);

impl FromRequestParts<AppState> for ClientIp {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Infallible> {
        if state.trust_proxy {
            let forwarded = parts
                .headers
                .get_all("x-forwarded-for")
                .iter()
                .filter_map(|v| v.to_str().ok())
                .flat_map(|v| v.split(','))
                .last()
                .and_then(|ip| ip.trim().parse().ok());
            if forwarded.is_some() {
                return Ok(Self(forwarded));
            }
        }
        Ok(Self(
            parts
                .extensions
                .get::<ConnectInfo<SocketAddr>>()
                .map(|info| info.0.ip()),
        ))
    }
}
