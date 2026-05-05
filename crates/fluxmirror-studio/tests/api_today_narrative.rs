// Phase 4 M-A2 — `/api/today` narrative wiring.
//
// Asserts the JSON response carries a `narrative` object whose
// `source = "heuristic"` whenever the AI provider is forced off.
// The default `AppState::new` fixture sets `provider="off"` so this
// test exercises the heuristic fallback without any network.

mod common;

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use serde_json::Value;
use tower::ServiceExt;

use fluxmirror_studio::build_router;

#[tokio::test]
async fn api_today_carries_heuristic_narrative_when_provider_off() {
    // 12-event today fixture. provider="off" by default in
    // `AppState::new`, which is what `fixture_today` uses, so the
    // synthesise path takes the heuristic branch immediately.
    let (_dir, state) = common::fixture_today(12);
    let app = build_router(state);

    let res = app
        .oneshot(Request::builder().uri("/api/today").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let v: Value = serde_json::from_slice(&body).unwrap();
    let narrative = v
        .get("narrative")
        .expect("response must carry a narrative field");

    assert_eq!(
        narrative.get("source").and_then(|s| s.as_str()),
        Some("heuristic"),
        "expected heuristic source, got: {narrative}"
    );
    let paragraph = narrative
        .get("paragraph")
        .and_then(|s| s.as_str())
        .unwrap_or("");
    assert!(
        !paragraph.is_empty(),
        "heuristic paragraph must be non-empty"
    );
    // No model field for heuristic; cost_usd should be 0.
    assert_eq!(
        narrative.get("cost_usd").and_then(|c| c.as_f64()),
        Some(0.0),
        "heuristic narrative must have cost_usd = 0.0"
    );
    assert_eq!(
        narrative.get("cache_hit").and_then(|c| c.as_bool()),
        Some(false)
    );
}

#[tokio::test]
async fn api_today_empty_window_still_returns_heuristic_narrative() {
    // Empty DB. The collector returns total_events = 0 and
    // synthesise_daily takes the empty-window branch directly to
    // heuristic.
    let (_dir, state) = common::fixture(&[]);
    let app = build_router(state);

    let res = app
        .oneshot(Request::builder().uri("/api/today").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let v: Value = serde_json::from_slice(&body).unwrap();
    let narrative = v.get("narrative").expect("must include narrative");
    assert_eq!(
        narrative.get("source").and_then(|s| s.as_str()),
        Some("heuristic")
    );
    let paragraph = narrative
        .get("paragraph")
        .and_then(|s| s.as_str())
        .unwrap_or("");
    assert!(
        paragraph.to_lowercase().contains("no agent activity"),
        "empty-window paragraph should call out the lack of activity, got: {paragraph}"
    );
}
