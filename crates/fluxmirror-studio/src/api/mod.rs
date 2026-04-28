//! Studio JSON API.
//!
//! Phase 3 M2 surface: `/api/today`, `/api/week`, `/api/now`. M3 adds
//! `/api/file?path=…` and `/api/file/git?path=…` for the per-file
//! provenance timeline. Every handler resolves a window via
//! `fluxmirror-core::report::data` and returns the canonical DTO as
//! JSON. The studio frontend is the primary consumer; downstream
//! tooling can roundtrip the same shape since the DTOs derive
//! `serde::Deserialize` too.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json, Router,
};

use crate::AppState;

pub mod file;
pub mod now;
pub mod today;
pub mod week;

/// Mount every API route under a single `/api` nest. Caller wires this
/// into the top-level router with `.nest("/api", api::router())`.
pub fn router() -> Router<AppState> {
    Router::new()
        .merge(today::router())
        .merge(week::router())
        .merge(now::router())
        .merge(file::router())
}

/// Canonical IANA tz used by the studio. The capture binary writes
/// every event in UTC; the studio renders today/week windows in the
/// configured timezone. M2 ships UTC only; the `?tz=` query parameter
/// will land alongside the settings page in M9.
pub(crate) const DEFAULT_TZ: &str = "UTC";

/// Shared `{ "error": "..." }` JSON body used by every handler when a
/// SQL or window-resolution failure aborts the request. Keeps the
/// frontend's error path predictable across routes.
pub(crate) fn error_response(status: StatusCode, msg: String) -> Response {
    (status, Json(serde_json::json!({ "error": msg }))).into_response()
}
