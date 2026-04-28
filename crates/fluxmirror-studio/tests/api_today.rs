// Integration test for `/api/today`.
//
// Spins up the same axum router the binary builds, hits `/api/today`
// via `tower::ServiceExt::oneshot` (no real HTTP socket needed), and
// asserts the JSON shape matches `fluxmirror_core::report::dto::TodayData`.

mod common;

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use serde_json::Value;
use tower::ServiceExt;

use fluxmirror_studio::build_router;

#[tokio::test]
async fn api_today_returns_dto_shape_for_busy_fixture() {
    let (_dir, state) = common::fixture_today(12);
    let app = build_router(state);

    let res = app
        .oneshot(Request::builder().uri("/api/today").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let v: Value = serde_json::from_slice(&body).unwrap();

    // Top-level shape: every key the frontend reads must be present.
    for key in [
        "date",
        "tz",
        "total_events",
        "agents",
        "files_edited",
        "files_read",
        "shells",
        "cwds",
        "mcp_methods",
        "tool_mix",
        "hours",
        "writes_total",
        "reads_total",
        "distinct_files",
    ] {
        assert!(v.get(key).is_some(), "missing /api/today key: {key}");
    }

    assert_eq!(v["total_events"].as_u64().unwrap(), 12);
    assert_eq!(v["hours"].as_array().unwrap().len(), 24);
    assert_eq!(v["agents"].as_array().unwrap()[0]["agent"], "claude-code");
}

#[tokio::test]
async fn api_today_returns_zero_total_for_empty_db() {
    let (_dir, state) = common::fixture(&[]);
    let app = build_router(state);

    let res = app
        .oneshot(Request::builder().uri("/api/today").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let v: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["total_events"].as_u64().unwrap(), 0);
    assert!(v["agents"].as_array().unwrap().is_empty());
    assert_eq!(v["hours"].as_array().unwrap().len(), 24);
}
