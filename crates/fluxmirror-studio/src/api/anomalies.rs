//! `/api/anomalies` — Phase 4 M-A6.
//!
//! Walks the heuristic detector in
//! `fluxmirror_core::report::anomaly`, then wraps each detection with
//! `fluxmirror_ai::synthesise_anomaly` to produce a one-sentence
//! `AnomalyStory`. The detector itself is cheap to re-run; the LLM
//! upgrade is the only expensive step, so the response cache below is
//! sized for that bottleneck — 1 hour TTL keyed on `(window, db_path)`.
//!
//! `?window=today` (default) and `?window=week` are the only valid
//! values. Anything else returns 400 so a typo can't silently land on
//! the wrong window.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration as StdDuration, Instant};

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::Deserialize;

use fluxmirror_ai::synthesise_anomaly;
use fluxmirror_core::report::anomaly::{detect_anomalies, AnomalyWindow};
use fluxmirror_core::report::dto::AnomalyStory;

use super::error_response;
use crate::AppState;

/// In-memory response TTL for the wrapped story list. Detector pass is
/// cheap, but the LLM call is not — cap at 1 hour so studio reloads
/// don't burn budget.
const CACHE_TTL: StdDuration = StdDuration::from_secs(60 * 60);

/// Process-local cache. Single-user studio + short handlers ⇒ a plain
/// `Mutex` is fine; no need to reach for a connection pool.
static RESPONSE_CACHE: Mutex<Option<CacheEntry>> = Mutex::new(None);

struct CacheEntry {
    window: AnomalyWindow,
    db_path: PathBuf,
    inserted: Instant,
    body: Vec<AnomalyStory>,
}

pub fn router() -> Router<AppState> {
    Router::new().route("/anomalies", get(handler))
}

#[derive(Debug, Deserialize)]
struct AnomaliesQuery {
    window: Option<String>,
}

async fn handler(
    State(state): State<AppState>,
    Query(q): Query<AnomaliesQuery>,
) -> Response {
    let window = match q.window.as_deref().unwrap_or("today") {
        "today" => AnomalyWindow::Today,
        "week" => AnomalyWindow::Week,
        other => {
            return error_response(
                StatusCode::BAD_REQUEST,
                format!("invalid window (expected today|week): {other}"),
            );
        }
    };

    if let Some(hit) = cache_lookup(&state.db_path, window) {
        return Json(hit).into_response();
    }

    let detections = {
        let conn = match state.db.lock() {
            Ok(c) => c,
            Err(e) => {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("db mutex poisoned: {e}"),
                );
            }
        };
        match detect_anomalies(&conn, state.config.as_ref(), window) {
            Ok(d) => d,
            Err(e) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, e),
        }
    };

    // Wrap each detection with an LLM- or heuristic-generated story.
    // Order is preserved so the UI can render the most-significant
    // anomaly (the first one returned by the detector) at the top.
    let store = state.ai_store.as_deref();
    let stories: Vec<AnomalyStory> = detections
        .iter()
        .map(|d| synthesise_anomaly(store, state.config.as_ref(), d))
        .collect();

    cache_insert(&state.db_path, window, stories.clone());
    Json(stories).into_response()
}

fn cache_lookup(db_path: &PathBuf, window: AnomalyWindow) -> Option<Vec<AnomalyStory>> {
    let guard = RESPONSE_CACHE.lock().ok()?;
    let entry = guard.as_ref()?;
    if entry.window != window {
        return None;
    }
    if &entry.db_path != db_path {
        return None;
    }
    if entry.inserted.elapsed() > CACHE_TTL {
        return None;
    }
    Some(entry.body.clone())
}

fn cache_insert(db_path: &PathBuf, window: AnomalyWindow, body: Vec<AnomalyStory>) {
    if let Ok(mut guard) = RESPONSE_CACHE.lock() {
        *guard = Some(CacheEntry {
            window,
            db_path: db_path.clone(),
            inserted: Instant::now(),
            body,
        });
    }
}
