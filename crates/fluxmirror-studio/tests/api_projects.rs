// Phase 4 M-A4 — `/api/projects` integration coverage.
//
// Builds a minimal events.db, mounts the studio router, and asserts
// that with `Config.ai.provider == "off"` (the default in the test
// environment, since no ANTHROPIC_API_KEY is wired) the endpoint
// returns heuristic-only projects with `source: "heuristic"` and
// JSON serialises cleanly.

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

/// Fixture: two clusters in the trailing 14 days, one of which spans
/// two sessions in the same cwd. The endpoint should fold them into
/// two distinct projects regardless of the LLM provider being off.
fn projects_fixture() -> (TempDir, AppState) {
    let now = Utc::now();
    let dir = tempfile::tempdir().unwrap();
    let path: PathBuf = dir.path().join("events.db");
    {
        let conn = Connection::open(&path).unwrap();
        common::schema(&conn);

        let insert = |ts: chrono::DateTime<Utc>, tool: &str, detail: &str, cwd: &str| {
            let ts_str = ts.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
            conn.execute(
                "INSERT INTO agent_events \
                 (ts, agent, session, tool, tool_canonical, tool_class, detail, \
                  cwd, host, user, schema_version, raw_json) \
                 VALUES (?1, ?2, ?3, ?4, ?4, 'Other', ?5, ?6, 'h', 'u', 1, '{}')",
                params![ts_str, "claude-code", "s", tool, detail, cwd],
            )
            .unwrap();
        };

        // Project A — single Building session ~5h ago in /proj/a.
        let a_start = now - Duration::hours(5);
        for i in 0..6 {
            insert(a_start + Duration::minutes(i), "Edit", "src/a.rs", "/proj/a");
        }

        // Project B — two sessions in /proj/b separated by ~3 days.
        let b1_start = now - Duration::days(4);
        for i in 0..6 {
            insert(b1_start + Duration::minutes(i), "Edit", "src/b1.rs", "/proj/b");
        }
        let b2_start = now - Duration::days(1) - Duration::hours(2);
        for i in 0..6 {
            insert(b2_start + Duration::minutes(i), "Edit", "src/b2.rs", "/proj/b");
        }
    }
    let ro = Connection::open_with_flags(
        &path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .unwrap();
    let state = AppState {
        db: Arc::new(Mutex::new(ro)),
        db_path: path,
    };
    (dir, state)
}

#[tokio::test]
async fn api_projects_returns_heuristic_only_when_provider_off() {
    // Drop ANTHROPIC_API_KEY so the LLM upgrade path falls into the
    // heuristic branch with `AiError::ProviderUnreachable`. The
    // endpoint must keep returning the cluster list with
    // `source: "heuristic"` even when the provider can't be reached.
    std::env::remove_var("ANTHROPIC_API_KEY");

    let (_dir, state) = projects_fixture();
    let app = build_router(state);

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/projects?days_back=14")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let bytes = to_bytes(res.into_body(), 10 * 1024 * 1024).await.unwrap();
    let body: Value = serde_json::from_slice(&bytes).expect("json parses");
    let arr = body.as_array().expect("top-level array");
    assert!(arr.len() >= 1, "expected at least one cluster, got {arr:?}");

    for project in arr {
        let obj = project.as_object().expect("each project is an object");
        assert_eq!(
            obj.get("source").and_then(|v| v.as_str()),
            Some("heuristic"),
            "provider=off must not upgrade to llm; got {project:?}"
        );
        // Required keys present.
        for key in [
            "id", "name", "arc", "status", "session_ids", "start", "end",
            "total_events", "total_usd", "dominant_cwd",
        ] {
            assert!(obj.contains_key(key), "missing key: {key}");
        }
        // Status is a known variant.
        let status = obj.get("status").and_then(|v| v.as_str()).unwrap();
        assert!(
            matches!(status, "active" | "paused" | "shipped" | "abandoned"),
            "unexpected status: {status}"
        );
    }
}

#[tokio::test]
async fn api_projects_invalid_days_back_is_400() {
    let (_dir, state) = projects_fixture();
    let app = build_router(state);

    let res = app
        .oneshot(
            Request::builder()
                .uri("/api/projects?days_back=-1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn api_projects_default_window_is_30() {
    std::env::set_var("FLUXMIRROR_AI_PROVIDER", "off");
    let (_dir, state) = projects_fixture();
    let app = build_router(state);

    let res = app
        .oneshot(
            Request::builder()
                .uri("/api/projects")
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
