//! The web UI (`web/dist`), embedded into the binary at build time.

use axum::{
    http::{header, StatusCode, Uri},
    response::{IntoResponse, Response},
    Json,
};
use rust_embed::Embed;
use serde_json::json;

#[derive(Embed)]
#[folder = "../web/dist"]
#[allow_missing = true]
struct Assets;

pub fn serve(uri: &Uri) -> Response {
    let path = match uri.path().trim_start_matches('/') {
        "" => "index.html",
        p => p,
    };
    let Some(file) = Assets::get(path) else {
        return (StatusCode::NOT_FOUND, Json(json!({ "error": "not found" }))).into_response();
    };
    // Vite fingerprints everything under assets/, so those never change; index.html always may.
    let cache = if path.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    (
        [
            (header::CONTENT_TYPE, file.metadata.mimetype().to_string()),
            (header::CACHE_CONTROL, cache.to_string()),
        ],
        file.data,
    )
        .into_response()
}
