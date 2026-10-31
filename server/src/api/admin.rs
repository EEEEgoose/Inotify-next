//! System settings and send statistics (administrators only).

use crate::{
    auth::AdminUser,
    channels::Kind,
    error::{AppError, AppResult},
    settings::{self, Settings},
    AppState,
};
use axum::{
    extract::{Query, State},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

/// Settings for the form. Secrets are write-only: they come back empty, with a flag.
fn settings_view(settings: &Settings) -> Value {
    let mut view = serde_json::to_value(settings).expect("settings serialize");
    view["turnstile_secret_key"] = "".into();
    view["turnstile_secret_key_set"] = (!settings.turnstile_secret_key.is_empty()).into();
    view
}

pub async fn get_settings(State(state): State<AppState>, _: AdminUser) -> Json<Value> {
    Json(settings_view(&state.settings()))
}

pub async fn put_settings(
    State(state): State<AppState>,
    _: AdminUser,
    Json(mut new): Json<Settings>,
) -> AppResult<Json<Value>> {
    let old = state.settings();
    if new.turnstile_secret_key.is_empty() {
        new.turnstile_secret_key = old.turnstile_secret_key;
    }
    new.turnstile_site_key = new.turnstile_site_key.trim().to_string();
    new.turnstile_secret_key = new.turnstile_secret_key.trim().to_string();
    new.proxy = new.proxy.trim().to_string();
    if new.turnstile_enabled && (new.turnstile_site_key.is_empty() || new.turnstile_secret_key.is_empty()) {
        return Err(AppError::BadRequest(
            "启用人机验证需要填写 Site Key 和 Secret Key".into(),
        ));
    }
    state
        .send_ctx
        .net
        .set_proxy(&new.proxy)
        .map_err(|e| AppError::BadRequest(format!("{e:#}")))?;
    settings::save(&state.db, &new).await?;
    *state.settings.write().unwrap() = new.clone();
    Ok(Json(settings_view(&new)))
}

#[derive(Deserialize)]
pub struct StatsQuery {
    days: Option<i64>,
}

pub async fn stats(State(state): State<AppState>, _: AdminUser, Query(q): Query<StatsQuery>) -> AppResult<Json<Value>> {
    let days = q.days.unwrap_or(30).clamp(1, 366);
    let since = (chrono::Local::now() - chrono::Duration::days(days - 1))
        .format("%Y-%m-%d")
        .to_string();
    let by_day: Vec<(String, i64, i64)> = sqlx::query_as(
        "SELECT date, SUM(success), SUM(failed) FROM send_stats WHERE date >= ? GROUP BY date ORDER BY date",
    )
    .bind(&since)
    .fetch_all(&state.db)
    .await?;
    let by_kind: Vec<(String, i64, i64)> = sqlx::query_as(
        "SELECT kind, SUM(success), SUM(failed) FROM send_stats WHERE date >= ? GROUP BY kind ORDER BY SUM(success) DESC",
    )
    .bind(&since)
    .fetch_all(&state.db)
    .await?;
    let totals: (i64, i64) = sqlx::query_as("SELECT COUNT(*), (SELECT COUNT(*) FROM channels) FROM users")
        .fetch_one(&state.db)
        .await?;

    Ok(Json(json!({
        "days": by_day.into_iter().map(|(date, success, failed)| json!({ "date": date, "success": success, "failed": failed })).collect::<Vec<_>>(),
        "kinds": by_kind.into_iter().map(|(kind, success, failed)| {
            let name = Kind::parse(&kind).map_or(kind.clone(), |k| k.info().name.to_string());
            json!({ "kind": kind, "name": name, "success": success, "failed": failed })
        }).collect::<Vec<_>>(),
        "users": totals.0,
        "channels": totals.1,
    })))
}
