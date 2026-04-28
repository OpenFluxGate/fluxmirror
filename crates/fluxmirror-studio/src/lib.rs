//! Library surface of fluxmirror-studio.
//!
//! Phase 3 M2 split the binary entrypoint into a thin `main.rs` plus
//! this library so integration tests under `tests/` can build the
//! same axum router (and the same [`AppState`]) without spawning the
//! real process or reaching into private modules.

pub mod api;
pub mod embed;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::{
    body::Body,
    extract::State,
    http::{header, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use rusqlite::Connection;

/// Per-request handler context. Owns a single read-only SQLite handle
/// guarded by a mutex — the studio is single-user and the dashboard
/// fetches are short, so contention is a non-issue and the simpler
/// `Mutex<Connection>` shape lets us avoid pulling in a connection
/// pool crate.
#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Mutex<Connection>>,
    pub db_path: PathBuf,
}

/// Build the full axum router. Mirrors the binary entrypoint exactly
/// so integration tests exercise the same wiring the user hits.
pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .nest("/api", api::router())
        .fallback(static_handler)
        .with_state(state)
}

/// `/health` — quick liveness + DB stats. Read-only; never writes.
pub async fn health(State(state): State<AppState>) -> impl IntoResponse {
    let agent_events: i64 = {
        let db = state.db.lock().expect("db mutex poisoned");
        db.query_row("SELECT COUNT(*) FROM agent_events", [], |row| row.get(0))
            .unwrap_or(0)
    };
    let proxy_events: i64 = {
        let db = state.db.lock().expect("db mutex poisoned");
        db.query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))
            .unwrap_or(0)
    };
    axum::Json(serde_json::json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
        "db": state.db_path.display().to_string(),
        "agent_events": agent_events,
        "proxy_events": proxy_events,
    }))
}

/// Serve embedded Vite assets, falling back to index.html for SPA
/// routes. Tests usually skip this path; it's exercised only when the
/// frontend is built into the binary.
pub async fn static_handler(uri: Uri) -> Response {
    let path = uri.path();

    if let Some((bytes, mime)) = embed::lookup(path) {
        return Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, mime)
            .header(header::CACHE_CONTROL, "public, max-age=3600")
            .body(Body::from(bytes))
            .expect("static response build");
    }

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Body::from(embed::index_html()))
        .expect("index response build")
}
