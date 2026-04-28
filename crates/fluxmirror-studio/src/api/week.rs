//! `/api/week` — 7-day rolling snapshot scoped to the configured
//! timezone. Calls `fluxmirror_core::report::data::collect_week`.

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use chrono::{Datelike, Duration, TimeZone, Utc};
use chrono_tz::Tz;
use std::str::FromStr;

use fluxmirror_core::report::data;
use fluxmirror_core::report::dto::WindowRange;

use super::{error_response, DEFAULT_TZ};
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/week", get(handler))
}

async fn handler(State(state): State<AppState>) -> Response {
    let tz: Tz = match Tz::from_str(DEFAULT_TZ) {
        Ok(t) => t,
        Err(e) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, format!("tz: {e}")),
    };

    let now_local = Utc::now().with_timezone(&tz);
    let today = now_local.date_naive();
    let tomorrow = today + Duration::days(1);
    let start_date = tomorrow - Duration::days(7);

    let start_local = match tz
        .with_ymd_and_hms(start_date.year(), start_date.month(), start_date.day(), 0, 0, 0)
        .single()
    {
        Some(t) => t,
        None => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("cannot resolve local midnight for {start_date} in {tz}"),
            );
        }
    };
    let end_local = match tz
        .with_ymd_and_hms(tomorrow.year(), tomorrow.month(), tomorrow.day(), 0, 0, 0)
        .single()
    {
        Some(t) => t,
        None => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("cannot resolve local midnight for {tomorrow} in {tz}"),
            );
        }
    };

    let range = WindowRange {
        start_utc: start_local.with_timezone(&Utc),
        end_utc: end_local.with_timezone(&Utc),
        anchor_date: start_date,
        tz: tz.name().to_string(),
    };

    let data_result = {
        let conn = match state.db.lock() {
            Ok(c) => c,
            Err(e) => {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("db mutex poisoned: {e}"),
                );
            }
        };
        data::collect_week(&conn, &tz, range, None)
    };
    match data_result {
        Ok(d) => Json(d).into_response(),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}
