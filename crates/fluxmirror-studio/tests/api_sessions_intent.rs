// Phase 4 M-A3 — `/api/sessions` JSON shape with the intent overlay.
//
// Confirms two contracts:
//   1. The list endpoint never errors when AI is off; the `intent`
//      key is simply absent from the wire shape (`skip_serializing_if`
//      on the DTO).
//   2. `Session` deserialises both shapes — pre-M-A3 (no `intent`) and
//      post-M-A3 (with `intent`) — so older snapshots stay readable.

mod common;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use chrono::{Duration, Utc};
use rusqlite::{params, Connection, OpenFlags};
use serde_json::Value;
use tempfile::TempDir;
use tower::ServiceExt;

use fluxmirror_core::report::dto::Session;
use fluxmirror_studio::{build_router, AppState};

/// One keepable cluster — small enough to keep the test fast, big
/// enough to satisfy the duration + event-count floor inside
/// `keep_cluster`.
fn fixture() -> (TempDir, AppState) {
    let now = Utc::now();
    let dir = tempfile::tempdir().unwrap();
    let path: PathBuf = dir.path().join("events.db");
    {
        let conn = Connection::open(&path).unwrap();
        common::schema(&conn);
        let s_start = now - Duration::hours(2);
        for i in 0..6 {
            let ts = (s_start + Duration::minutes(i * 2))
                .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
            conn.execute(
                "INSERT INTO agent_events \
                 (ts, agent, session, tool, tool_canonical, tool_class, detail, \
                  cwd, host, user, schema_version, raw_json) \
                 VALUES (?1, 'claude-code', 's1', 'Edit', 'Edit', 'Write', \
                         'src/lib.rs', '/proj/intent', 'h', 'u', 1, '{}')",
                params![ts],
            )
            .unwrap();
        }
    }
    let ro = Connection::open_with_flags(
        &path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .unwrap();
    // `AppState::new` defaults to provider="off" + no AI store, which
    // is exactly the path we want this test to exercise.
    let state = AppState::new(Arc::new(Mutex::new(ro)), path);
    (dir, state)
}

#[tokio::test]
async fn api_sessions_intent_key_absent_when_provider_off() {
    let (_d, state) = fixture();
    let app = build_router(state);

    let res = app
        .oneshot(
            Request::builder()
                .uri("/api/sessions")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let v: Value = serde_json::from_slice(&body).unwrap();
    let arr = v.as_array().expect("array");
    assert!(!arr.is_empty(), "fixture should produce at least one session");
    for s in arr {
        // Heuristic surface still works.
        assert!(s.get("name").and_then(Value::as_str).is_some());
        // `skip_serializing_if = "Option::is_none"` keeps the key off
        // the wire when AI is off.
        assert!(
            s.get("intent").is_none(),
            "intent should be elided when provider=off, got: {s}"
        );
    }
}

#[tokio::test]
async fn session_struct_deserialises_with_or_without_intent() {
    // The TS shape is permissive (`intent?: string`). The Rust DTO
    // matches that — both wire shapes parse into the same `Session`
    // type without error.
    let without = r#"{
        "id": "11111111",
        "start": "2026-04-26T10:00:00Z",
        "end": "2026-04-26T10:30:00Z",
        "agents": ["claude-code"],
        "event_count": 6,
        "dominant_cwd": "/proj/x",
        "top_files": ["src/lib.rs"],
        "tool_mix": [],
        "lifecycle": "building",
        "name": "Built: x (Edit-heavy, 1 files)",
        "events": []
    }"#;
    let s_without: Session = serde_json::from_str(without).unwrap();
    assert!(s_without.intent.is_none());

    let with = r#"{
        "id": "22222222",
        "start": "2026-04-26T10:00:00Z",
        "end": "2026-04-26T10:30:00Z",
        "agents": ["claude-code"],
        "event_count": 6,
        "dominant_cwd": "/proj/x",
        "top_files": ["src/lib.rs"],
        "tool_mix": [],
        "lifecycle": "building",
        "name": "Built: x (Edit-heavy, 1 files)",
        "intent": "Polishing the cache invalidation",
        "events": []
    }"#;
    let s_with: Session = serde_json::from_str(with).unwrap();
    assert_eq!(
        s_with.intent.as_deref(),
        Some("Polishing the cache invalidation")
    );
}

#[tokio::test]
async fn api_session_detail_intent_key_absent_when_provider_off() {
    let (_d, state) = fixture();
    // First fetch the list so we know a real session id to look up.
    let app = build_router(state.clone());
    let res = app
        .oneshot(
            Request::builder()
                .uri("/api/sessions")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let list: Vec<Value> = serde_json::from_slice(&body).unwrap();
    let id = list
        .first()
        .and_then(|s| s.get("id"))
        .and_then(Value::as_str)
        .expect("at least one session id");
    let app2 = build_router(state);
    let res2 = app2
        .oneshot(
            Request::builder()
                .uri(format!("/api/session/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res2.status(), StatusCode::OK);
    let body2 = to_bytes(res2.into_body(), usize::MAX).await.unwrap();
    let detail: Value = serde_json::from_slice(&body2).unwrap();
    assert!(
        detail.get("intent").is_none(),
        "intent should be elided on detail too, got: {detail}"
    );
}
