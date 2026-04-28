// Integration test for `/api/week`.
//
// Same harness as api_today: build the router, dispatch a single
// request via tower's oneshot helper, assert JSON shape.

mod common;

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use serde_json::Value;
use tower::ServiceExt;

use fluxmirror_studio::build_router;

#[tokio::test]
async fn api_week_returns_dto_shape_for_busy_fixture() {
    let (_dir, state) = common::fixture_today(8);
    let app = build_router(state);

    let res = app
        .oneshot(Request::builder().uri("/api/week").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let v: Value = serde_json::from_slice(&body).unwrap();

    for key in [
        "range_start",
        "range_end",
        "tz",
        "total_events",
        "agents",
        "files_edited",
        "files_read",
        "cwds",
        "tool_mix",
        "daily",
        "heatmap",
        "shell_counts",
        "mcp_count",
        "writes_total",
        "reads_total",
    ] {
        assert!(v.get(key).is_some(), "missing /api/week key: {key}");
    }

    // Daily list always covers exactly 7 chronological days, even when
    // some have zero events.
    let daily = v["daily"].as_array().unwrap();
    assert_eq!(daily.len(), 7);
    let total: u64 = daily
        .iter()
        .map(|d| d["calls"].as_u64().unwrap())
        .sum();
    assert_eq!(total, v["total_events"].as_u64().unwrap());

    // Heatmap is a 7×24 matrix.
    let heatmap = v["heatmap"].as_array().unwrap();
    assert_eq!(heatmap.len(), 7);
    for row in heatmap {
        assert_eq!(row.as_array().unwrap().len(), 24);
    }
}

#[tokio::test]
async fn api_week_returns_zero_total_for_empty_db() {
    let (_dir, state) = common::fixture(&[]);
    let app = build_router(state);

    let res = app
        .oneshot(Request::builder().uri("/api/week").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let v: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["total_events"].as_u64().unwrap(), 0);
    assert_eq!(v["daily"].as_array().unwrap().len(), 7);
    assert_eq!(v["heatmap"].as_array().unwrap().len(), 7);
}
