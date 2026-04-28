//! `/api/sessions` and `/api/session/:id` — Phase 3 M5.
//!
//! `/api/sessions?from=YYYY-MM-DD&to=YYYY-MM-DD` returns the heuristic
//! work sessions that fit inside the requested local window. Both
//! params default to a 7-day trailing window anchored on "now in
//! `DEFAULT_TZ`".
//!
//! `/api/session/:id` re-derives every session from the trailing
//! 30 days and returns the matching one (with the per-event timeline
//! populated). 404 when no session matches.

use std::str::FromStr;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use chrono::{Datelike, Duration, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;
use serde::Deserialize;

use fluxmirror_core::report::dto::WindowRange;
use fluxmirror_core::report::sessions;

use super::{error_response, DEFAULT_TZ};
use crate::AppState;

/// Default trailing window for the list endpoint when the caller
/// doesn't pin one with `?from=` / `?to=`.
const DEFAULT_LIST_DAYS: i64 = 7;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/sessions", get(list_handler))
        .route("/session/:id", get(detail_handler))
}

#[derive(Debug, Deserialize)]
struct SessionsQuery {
    from: Option<String>,
    to: Option<String>,
}

async fn list_handler(
    State(state): State<AppState>,
    Query(q): Query<SessionsQuery>,
) -> Response {
    let tz: Tz = match Tz::from_str(DEFAULT_TZ) {
        Ok(t) => t,
        Err(e) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, format!("tz: {e}")),
    };

    let now_local = Utc::now().with_timezone(&tz);
    let today = now_local.date_naive();
    let tomorrow = today + Duration::days(1);
    let default_from = tomorrow - Duration::days(DEFAULT_LIST_DAYS);

    let from_date = match q.from.as_deref() {
        Some(s) => match NaiveDate::parse_from_str(s, "%Y-%m-%d") {
            Ok(d) => d,
            Err(_) => {
                return error_response(
                    StatusCode::BAD_REQUEST,
                    format!("invalid `from` (expected YYYY-MM-DD): {s}"),
                );
            }
        },
        None => default_from,
    };
    let to_date = match q.to.as_deref() {
        Some(s) => match NaiveDate::parse_from_str(s, "%Y-%m-%d") {
            Ok(d) => d,
            Err(_) => {
                return error_response(
                    StatusCode::BAD_REQUEST,
                    format!("invalid `to` (expected YYYY-MM-DD): {s}"),
                );
            }
        },
        None => tomorrow,
    };

    if to_date <= from_date {
        return error_response(
            StatusCode::BAD_REQUEST,
            format!("`to` must be after `from` (got from={from_date} to={to_date})"),
        );
    }

    let start_local = match tz
        .with_ymd_and_hms(from_date.year(), from_date.month(), from_date.day(), 0, 0, 0)
        .single()
    {
        Some(t) => t,
        None => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("cannot resolve local midnight for {from_date} in {tz}"),
            );
        }
    };
    let end_local = match tz
        .with_ymd_and_hms(to_date.year(), to_date.month(), to_date.day(), 0, 0, 0)
        .single()
    {
        Some(t) => t,
        None => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("cannot resolve local midnight for {to_date} in {tz}"),
            );
        }
    };

    let range = WindowRange {
        start_utc: start_local.with_timezone(&Utc),
        end_utc: end_local.with_timezone(&Utc),
        anchor_date: from_date,
        tz: tz.name().to_string(),
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
        sessions::collect_sessions(&conn, &tz, range)
    };
    match result {
        Ok(list) => Json(list).into_response(),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

async fn detail_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    if id.is_empty() {
        return error_response(StatusCode::BAD_REQUEST, "missing session id".to_string());
    }
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
        sessions::collect_session_detail(&conn, &id)
    };
    match result {
        Ok(Some(session)) => Json(session).into_response(),
        Ok(None) => error_response(
            StatusCode::NOT_FOUND,
            format!("session not found: {id}"),
        ),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}
