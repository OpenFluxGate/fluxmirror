//! `/api/replay` — scrubbable per-day timeline.
//!
//! Two endpoints sit behind this module:
//!
//! - `GET /api/replay/:date` returns every event in the local day plus
//!   a 1440-entry minute-bucket vector that the frontend renders as a
//!   24-hour heatmap-as-slider.
//! - `GET /api/replay/:date/at?ts=<iso>` returns the live state at a
//!   specific instant — the active file, the most recent five events,
//!   and per-minute agent + tool histograms.
//!
//! Both routes resolve days in the studio's configured timezone (UTC
//! until M9 lands the settings page) so day boundaries match what the
//! today and week pages render.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use chrono::{DateTime, NaiveDate, Utc};
use chrono_tz::Tz;
use serde::Deserialize;
use std::str::FromStr;

use fluxmirror_core::report::data;

use super::{error_response, DEFAULT_TZ};
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/replay/:date", get(day_handler))
        .route("/replay/:date/at", get(snapshot_handler))
}

#[derive(Debug, Deserialize)]
struct SnapshotQuery {
    /// RFC 3339 timestamp ("scrub position"). Required.
    ts: Option<String>,
}

async fn day_handler(State(state): State<AppState>, Path(date): Path<String>) -> Response {
    let tz = match resolve_tz() {
        Ok(t) => t,
        Err(e) => return e,
    };
    let parsed = match NaiveDate::parse_from_str(&date, "%Y-%m-%d") {
        Ok(d) => d,
        Err(e) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                format!("invalid date '{date}': {e} — expected YYYY-MM-DD"),
            );
        }
    };

    let result = {
        let conn = match state.db.lock() {
            Ok(c) => c,
            Err(e) => {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("db mutex poisoned: {e}"),
                );
            }
        };
        data::collect_replay_day(&conn, &tz, parsed)
    };
    match result {
        Ok(d) => Json(d).into_response(),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

async fn snapshot_handler(
    State(state): State<AppState>,
    Path(date): Path<String>,
    Query(q): Query<SnapshotQuery>,
) -> Response {
    let tz = match resolve_tz() {
        Ok(t) => t,
        Err(e) => return e,
    };
    let parsed_date = match NaiveDate::parse_from_str(&date, "%Y-%m-%d") {
        Ok(d) => d,
        Err(e) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                format!("invalid date '{date}': {e} — expected YYYY-MM-DD"),
            );
        }
    };
    let ts_str = match q.ts.as_deref().filter(|s| !s.is_empty()) {
        Some(s) => s,
        None => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "missing required query parameter: ts".to_string(),
            );
        }
    };
    let ts: DateTime<Utc> = match DateTime::parse_from_rfc3339(ts_str) {
        Ok(dt) => dt.with_timezone(&Utc),
        Err(e) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                format!("invalid ts '{ts_str}': {e} — expected RFC 3339"),
            );
        }
    };

    let result = {
        let conn = match state.db.lock() {
            Ok(c) => c,
            Err(e) => {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("db mutex poisoned: {e}"),
                );
            }
        };
        data::collect_replay_snapshot(&conn, &tz, parsed_date, ts)
    };
    match result {
        Ok(s) => Json(s).into_response(),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

/// Resolve the studio's configured timezone. Centralised so both
/// handlers report the same error path.
fn resolve_tz() -> Result<Tz, Response> {
    Tz::from_str(DEFAULT_TZ)
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, format!("tz: {e}")))
}
