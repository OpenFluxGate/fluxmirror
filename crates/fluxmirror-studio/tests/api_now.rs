// Integration test for `/api/now`.
//
// Verifies the latest-event snapshot shape and that the `null` body
// is returned (not a 404) when the database has no rows.

mod common;

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use serde_json::Value;
use tower::ServiceExt;

use fluxmirror_studio::build_router;

#[tokio::test]
async fn api_now_returns_null_for_empty_db() {
    let (_dir, state) = common::fixture(&[]);
    let app = build_router(state);

    let res = app
        .oneshot(Request::builder().uri("/api/now").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let v: Value = serde_json::from_slice(&body).unwrap();
    assert!(v.is_null(), "expected JSON null for empty DB, got {v}");
}

#[tokio::test]
async fn api_now_returns_latest_event_payload() {
    let (_dir, state) = common::fixture(&[
        (
            "2026-04-26T01:00:00Z",
            "claude-code",
            "Edit",
            "s1",
            "src/a.rs",
            "/p",
        ),
        (
            "2026-04-26T02:30:00Z",
            "gemini-cli",
            "edit_file",
            "g1",
            "README.md",
            "/q",
        ),
        (
            "2026-04-26T02:35:00Z",
            "claude-code",
            "Bash",
            "s1",
            "ls",
            "/p",
        ),
    ]);
    let app = build_router(state);

    let res = app
        .oneshot(Request::builder().uri("/api/now").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let v: Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(v["latest_agent"], "claude-code");
    assert_eq!(v["latest_tool"], "Bash");
    assert_eq!(v["latest_detail"], "ls");
    assert_eq!(v["latest_cwd"], "/p");
    // The 02:30 gemini event and the 02:35 claude event both fall
    // inside the trailing 60-minute window from 02:35.
    assert_eq!(v["last_hour_total"].as_u64().unwrap(), 2);
    assert_eq!(v["last_hour_agents"].as_array().unwrap().len(), 2);
}
