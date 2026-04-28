//! `/api/now` — most-recent-event snapshot.
//!
//! Returns the latest `agent_events` row plus a per-agent breakdown of
//! the trailing 60 minutes around it. JSON is `null` when the database
//! has no rows yet.

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};

use fluxmirror_core::report::data;

use super::error_response;
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/now", get(handler))
}

async fn handler(State(state): State<AppState>) -> Response {
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
        data::collect_now(&conn)
    };
    match result {
        Ok(snap) => Json(snap).into_response(),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}
