// Integration tests for `/api/replay/:date` and
// `/api/replay/:date/at`.
//
// Spins up the same axum router the binary builds, drives it via
// `tower::ServiceExt::oneshot`, and asserts the JSON shape matches the
// `fluxmirror_core::report::dto::ReplayDay` and `ReplaySnapshot`
// structs.

mod common;

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use serde_json::Value;
use tower::ServiceExt;

use fluxmirror_studio::build_router;

const REPLAY_DATE: &str = "2026-04-26";

/// Fixture day with five events spread across UTC minutes
/// 9 + 14 + 14 + 14 + 22 of `REPLAY_DATE`. Plus one event the day
/// before and one the day after — both must be excluded from the
/// /api/replay/:date response.
fn replay_fixture() -> (tempfile::TempDir, fluxmirror_studio::AppState) {
    common::fixture(&[
        // Day before — must not appear in the day's events list.
        (
            "2026-04-25T23:59:30Z",
            "claude-code",
            "Edit",
            "s0",
            "src/yesterday.rs",
            "/proj/a",
        ),
        // Five in-day events.
        (
            "2026-04-26T00:09:00Z",
            "claude-code",
            "Read",
            "s1",
            "src/a.rs",
            "/proj/a",
        ),
        (
            "2026-04-26T00:14:10Z",
            "claude-code",
            "Edit",
            "s1",
            "src/foo.rs",
            "/proj/a",
        ),
        (
            "2026-04-26T00:14:30Z",
            "gemini-cli",
            "edit_file",
            "g1",
            "src/bar.rs",
            "/proj/a",
        ),
        (
            "2026-04-26T00:14:50Z",
            "claude-code",
            "Bash",
            "s1",
            "cargo test",
            "/proj/a",
        ),
        (
            "2026-04-26T00:22:00Z",
            "claude-code",
            "Edit",
            "s1",
            "src/baz.rs",
            "/proj/a",
        ),
        // Day after — must not appear.
        (
            "2026-04-27T00:00:30Z",
            "claude-code",
            "Edit",
            "s2",
            "src/tomorrow.rs",
            "/proj/a",
        ),
    ])
}

#[tokio::test]
async fn api_replay_day_returns_known_events() {
    let (_dir, state) = replay_fixture();
    let app = build_router(state);

    let res = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/replay/{REPLAY_DATE}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let v: Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(v["date"].as_str().unwrap(), REPLAY_DATE);
    let events = v["events"].as_array().unwrap();
    // The day-before and day-after events must be excluded.
    assert_eq!(events.len(), 5);

    // First event is the read at 00:09, last is the edit at 00:22.
    assert!(events[0]["ts"]
        .as_str()
        .unwrap()
        .starts_with("2026-04-26T00:09:00"));
    assert!(events[4]["ts"]
        .as_str()
        .unwrap()
        .starts_with("2026-04-26T00:22:00"));
}

#[tokio::test]
async fn api_replay_day_minute_buckets_are_full_1440() {
    let (_dir, state) = replay_fixture();
    let app = build_router(state);

    let res = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/replay/{REPLAY_DATE}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let v: Value = serde_json::from_slice(&body).unwrap();
    let buckets = v["minute_buckets"].as_array().unwrap();
    assert_eq!(buckets.len(), 1440);

    // Index check: entry N has minute == N.
    for (i, b) in buckets.iter().enumerate() {
        assert_eq!(b["minute"].as_u64().unwrap(), i as u64);
    }

    // Spot-check the loaded buckets: minute 9 = 1, minute 14 = 3,
    // minute 22 = 1. Every other minute is zero.
    assert_eq!(buckets[9]["count"].as_u64().unwrap(), 1);
    assert_eq!(buckets[14]["count"].as_u64().unwrap(), 3);
    assert_eq!(buckets[22]["count"].as_u64().unwrap(), 1);
    let total: u64 = buckets
        .iter()
        .map(|b| b["count"].as_u64().unwrap())
        .sum();
    assert_eq!(total, 5);
}

#[tokio::test]
async fn api_replay_day_rejects_garbage_date() {
    let (_dir, state) = replay_fixture();
    let app = build_router(state);
    let res = app
        .oneshot(
            Request::builder()
                .uri("/api/replay/not-a-date")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn api_replay_at_returns_active_file_in_window() {
    let (_dir, state) = replay_fixture();
    let app = build_router(state);
    // ts = 00:15:00 — minute 15. Trailing 60s window covers
    // 00:14:00..=00:15:00, which includes the 00:14 edits. The most
    // recent write event in that window is `gemini-cli edit_file
    // src/bar.rs` at 00:14:30 (the 00:14:50 event is a Bash call,
    // which doesn't count as a write). Actually wait — the Edit at
    // 00:14:10 is `claude-code` and is older. The most recent write
    // in the window is at 00:14:30 → `src/bar.rs`.
    let res = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/replay/{REPLAY_DATE}/at?ts=2026-04-26T00:15:00Z"
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let v: Value = serde_json::from_slice(&body).unwrap();

    assert!(v["at"].as_str().unwrap().starts_with("2026-04-26T00:15:00"));
    assert_eq!(v["active_file"].as_str().unwrap(), "src/bar.rs");

    // Three events in the trailing 60s: two edits + one bash.
    let agent_mix = v["agent_minute_mix"].as_array().unwrap();
    let total_calls: u64 = agent_mix
        .iter()
        .map(|a| a["calls"].as_u64().unwrap())
        .sum();
    assert_eq!(total_calls, 3);
    // last_n holds up to 5 most recent at-or-before ts (within day).
    let last_n = v["last_n_events"].as_array().unwrap();
    assert!(!last_n.is_empty());
    assert!(last_n.len() <= 5);
}

#[tokio::test]
async fn api_replay_at_no_active_file_when_quiet() {
    let (_dir, state) = replay_fixture();
    let app = build_router(state);
    // ts = 00:00:30 — within the day, before any in-day event. The
    // trailing 60s window slides into the day-before territory but the
    // collector clamps to the local day, so no events should be in
    // the window and active_file is null.
    let res = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/replay/{REPLAY_DATE}/at?ts=2026-04-26T00:00:30Z"
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let v: Value = serde_json::from_slice(&body).unwrap();
    assert!(v["active_file"].is_null());
    assert!(v["last_n_events"].as_array().unwrap().is_empty());
    assert!(v["agent_minute_mix"].as_array().unwrap().is_empty());
    assert!(v["tool_minute_mix"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn api_replay_at_requires_ts_param() {
    let (_dir, state) = replay_fixture();
    let app = build_router(state);
    let res = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/replay/{REPLAY_DATE}/at"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn api_replay_at_rejects_bad_ts() {
    let (_dir, state) = replay_fixture();
    let app = build_router(state);
    let res = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/replay/{REPLAY_DATE}/at?ts=not-an-iso"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn api_replay_day_empty_db_returns_zero_events_with_full_buckets() {
    let (_dir, state) = common::fixture(&[]);
    let app = build_router(state);
    let res = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/replay/{REPLAY_DATE}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let v: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["date"].as_str().unwrap(), REPLAY_DATE);
    assert_eq!(v["events"].as_array().unwrap().len(), 0);
    assert_eq!(v["minute_buckets"].as_array().unwrap().len(), 1440);
    let total: u64 = v["minute_buckets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["count"].as_u64().unwrap())
        .sum();
    assert_eq!(total, 0);
}
