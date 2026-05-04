// Integration tests for `/api/session/:id` — Phase 3 M5.
//
// The detail endpoint scans the trailing 30 days from `Utc::now()`,
// so the fixture rows are anchored on `now - duration` for stability
// across runs. We grab a known id off `/api/sessions` and assert that
// the detail payload populates the per-event timeline.

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

use fluxmirror_studio::{build_router, AppState};

/// Smaller fixture than the list test — just two sessions, both
/// inside the trailing 30-day window: one Building (5 edits) and one
/// Shipping (git tag + git push).
fn detail_fixture() -> (TempDir, AppState) {
    let now = Utc::now();
    let dir = tempfile::tempdir().unwrap();
    let path: PathBuf = dir.path().join("events.db");
    {
        let conn = Connection::open(&path).unwrap();
        common::schema(&conn);
        let mut insert = |ts: chrono::DateTime<Utc>,
                          agent: &str,
                          tool: &str,
                          session: &str,
                          detail: &str,
                          cwd: &str| {
            let ts_str = ts.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
            conn.execute(
                "INSERT INTO agent_events \
                 (ts, agent, session, tool, tool_canonical, tool_class, detail, \
                  cwd, host, user, schema_version, raw_json) \
                 VALUES (?1, ?2, ?3, ?4, ?4, 'Other', ?5, ?6, 'h', 'u', 1, '{}')",
                params![ts_str, agent, session, tool, detail, cwd],
            )
            .unwrap();
        };

        // Building session, ~3 hours ago.
        let s1_start = now - Duration::hours(3);
        for i in 0..5 {
            insert(
                s1_start + Duration::minutes(i * 2),
                "claude-code",
                "Edit",
                "s1",
                "src/build.rs",
                "/proj/build",
            );
        }

        // Shipping session, ~1 hour ago. Three Bash events: tag,
        // push tag, push main.
        let s2_start = now - Duration::hours(1);
        insert(
            s2_start,
            "claude-code",
            "Bash",
            "s2",
            "git tag v0.6.0",
            "/proj/ship",
        );
        insert(
            s2_start + Duration::minutes(1),
            "claude-code",
            "Bash",
            "s2",
            "git push origin v0.6.0",
            "/proj/ship",
        );
        insert(
            s2_start + Duration::minutes(2),
            "claude-code",
            "Bash",
            "s2",
            "git push origin main",
            "/proj/ship",
        );
        // Pad to keep the cluster (5 events, ≥5 minute span).
        insert(
            s2_start + Duration::minutes(4),
            "claude-code",
            "Bash",
            "s2",
            "git status",
            "/proj/ship",
        );
        insert(
            s2_start + Duration::minutes(5),
            "claude-code",
            "Bash",
            "s2",
            "git log -1",
            "/proj/ship",
        );
    }
    let ro = Connection::open_with_flags(
        &path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .unwrap();
    let state = AppState::new(Arc::new(Mutex::new(ro)), path);
    (dir, state)
}

async fn fetch_session_list(state: AppState) -> Vec<Value> {
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
    let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice::<Value>(&body)
        .unwrap()
        .as_array()
        .cloned()
        .unwrap_or_default()
}

#[tokio::test]
async fn api_session_detail_returns_events_for_known_id() {
    let (_d, state) = detail_fixture();
    let list = fetch_session_list(state.clone()).await;
    assert_eq!(list.len(), 2, "fixture should produce two sessions");

    // Pick the building session — first chronologically.
    let target = list
        .iter()
        .find(|s| s["lifecycle"] == "building")
        .expect("building session present");
    let id = target["id"].as_str().unwrap();

    let app = build_router(state);
    let res = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/session/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let v: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["id"], id);
    assert_eq!(v["lifecycle"], "building");
    let events = v["events"].as_array().expect("events populated");
    assert_eq!(events.len(), 5);
    for ev in events {
        assert_eq!(ev["agent"], "claude-code");
        assert_eq!(ev["tool"], "Edit");
        assert_eq!(ev["detail"], "src/build.rs");
    }
}

#[tokio::test]
async fn api_session_detail_404s_on_unknown_id() {
    let (_d, state) = detail_fixture();
    let app = build_router(state);
    let res = app
        .oneshot(
            Request::builder()
                .uri("/api/session/deadbeef")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn api_session_detail_shipping_extracts_tag_in_name() {
    let (_d, state) = detail_fixture();
    let list = fetch_session_list(state.clone()).await;
    let target = list
        .iter()
        .find(|s| s["lifecycle"] == "shipping")
        .expect("shipping session present");
    let id = target["id"].as_str().unwrap();

    let app = build_router(state);
    let res = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/session/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let v: Value = serde_json::from_slice(&body).unwrap();
    let name = v["name"].as_str().unwrap();
    assert!(
        name.starts_with("Shipped:"),
        "expected Shipped prefix, got {name}"
    );
    assert!(
        name.contains("v0.6.0"),
        "expected tag in tail, got {name}"
    );
}

#[tokio::test]
async fn api_session_detail_id_round_trips_through_list() {
    // Pull the list, take an id, fetch detail, assert the id round-trips
    // unchanged. Guards against any path-encoding shenanigans.
    let (_d, state) = detail_fixture();
    let list = fetch_session_list(state.clone()).await;
    for s in &list {
        let id = s["id"].as_str().unwrap();
        let app = build_router(state.clone());
        let res = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/session/{id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
        let v: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["id"], id);
        assert_eq!(v["start"], s["start"]);
        assert_eq!(v["end"], s["end"]);
        assert_eq!(v["name"], s["name"]);
    }
}
