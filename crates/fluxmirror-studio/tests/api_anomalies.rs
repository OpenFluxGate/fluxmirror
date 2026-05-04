// Phase 4 M-A6 — `/api/anomalies` integration coverage.
//
// Builds a fixture DB with an obvious file-edit spike, mounts the
// studio router, and asserts:
//   * the endpoint returns 200 with a JSON array,
//   * a FileEditSpike entry shows up,
//   * with `provider="off"` the source is `heuristic` and the story
//     string is non-empty.
//
// Also covers the validation path: an unknown `?window=` param must
// return 400 so the UI doesn't get a stale today response by accident.

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

/// Seed a today-anchored spike on a single file plus a much smaller
/// trailing baseline. The detector should fire FileEditSpike on this
/// shape with deterministic ordering.
fn spike_fixture() -> (TempDir, AppState) {
    let now = Utc::now();
    let dir = tempfile::tempdir().unwrap();
    let path: PathBuf = dir.path().join("events.db");
    {
        let conn = Connection::open(&path).unwrap();
        common::schema(&conn);

        let insert = |ts: chrono::DateTime<Utc>, agent: &str, tool: &str, detail: &str| {
            let ts_str = ts.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
            conn.execute(
                "INSERT INTO agent_events \
                 (ts, agent, session, tool, tool_canonical, tool_class, detail, \
                  cwd, host, user, schema_version, raw_json) \
                 VALUES (?1, ?2, 's', ?3, ?3, 'Other', ?4, '/p', 'h', 'u', 1, '{}')",
                params![ts_str, agent, tool, detail],
            )
            .unwrap();
        };

        // Today: 12 edits to the same file.
        for i in 0..12 {
            insert(
                now - Duration::minutes(i),
                "claude-code",
                "Edit",
                "Cargo.toml",
            );
        }
        // Baseline: a handful of edits across the trailing 28 days.
        for i in 1..=4 {
            insert(
                now - Duration::days(i),
                "claude-code",
                "Edit",
                "Cargo.toml",
            );
        }
    }
    let ro = Connection::open_with_flags(
        &path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .unwrap();
    let state = AppState::new(Arc::new(Mutex::new(ro)), path);
    (dir, state)
}

#[tokio::test]
async fn api_anomalies_returns_file_edit_spike_with_heuristic_source() {
    // Provider defaults to "off" via AppState::new, so the LLM upgrade
    // path is gated and every story should fall through to the
    // deterministic template branch.
    std::env::remove_var("ANTHROPIC_API_KEY");

    let (_d, state) = spike_fixture();
    let app = build_router(state);

    let res = app
        .oneshot(
            Request::builder()
                .uri("/api/anomalies?window=today")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let bytes = to_bytes(res.into_body(), 10 * 1024 * 1024).await.unwrap();
    let body: Value = serde_json::from_slice(&bytes).expect("json parses");
    let arr = body.as_array().expect("top-level array");
    assert!(
        !arr.is_empty(),
        "expected at least one anomaly, got empty list"
    );

    let spike = arr
        .iter()
        .find(|s| s.get("kind").and_then(Value::as_str) == Some("file_edit_spike"))
        .expect("FileEditSpike present in body");

    let obj = spike.as_object().unwrap();
    for key in [
        "kind", "story", "observed", "baseline", "evidence", "source",
    ] {
        assert!(obj.contains_key(key), "missing key: {key}");
    }
    assert_eq!(
        obj.get("source").and_then(Value::as_str),
        Some("heuristic"),
        "provider=off must keep source=heuristic; got: {spike}"
    );
    let story = obj.get("story").and_then(Value::as_str).unwrap_or("");
    assert!(!story.is_empty(), "story must be non-empty even on heuristic path");
    let evidence = obj.get("evidence").and_then(Value::as_array).unwrap();
    assert!(!evidence.is_empty(), "evidence list must be populated");
}

#[tokio::test]
async fn api_anomalies_default_window_is_today() {
    let (_d, state) = spike_fixture();
    let app = build_router(state);

    let res = app
        .oneshot(
            Request::builder()
                .uri("/api/anomalies")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = to_bytes(res.into_body(), 10 * 1024 * 1024).await.unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert!(body.is_array());
}

#[tokio::test]
async fn api_anomalies_invalid_window_is_400() {
    let (_d, state) = spike_fixture();
    let app = build_router(state);

    let res = app
        .oneshot(
            Request::builder()
                .uri("/api/anomalies?window=month")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}
