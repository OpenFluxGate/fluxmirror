// Integration tests for `/api/sessions` — Phase 3 M5.
//
// Builds a fixture DB anchored on `Utc::now()` so the trailing 7-day
// default window always covers the seeded events regardless of when
// the test suite runs. Assertions cover cluster count, name format,
// lifecycle classification, and end-to-end determinism.

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

/// Build a sessions-shaped fixture: three keepable clusters plus a
/// noise singleton that should be dropped by the duration+count
/// filter. Keeping every timestamp relative to `Utc::now()` makes the
/// fixture stable regardless of when the suite runs.
fn sessions_fixture() -> (TempDir, AppState) {
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

        // SESSION 3 — Polishing. 8 distinct files, 7-minute span,
        // ~10 hours ago. Earliest in the database, so it's emitted
        // first by the chronologically-ordered scan.
        let s3_start = now - Duration::hours(10);
        for i in 0..8 {
            insert(
                s3_start + Duration::minutes(i),
                "gemini-cli",
                "edit_file",
                "g3",
                &format!("src/m{}.rs", i),
                "/proj/c",
            );
        }

        // SESSION 2 — Testing. 6 cargo test cycles + 44 edits across
        // ~22 minutes, ~5 hours ago.
        let s2_start = now - Duration::hours(5);
        for i in 0..6 {
            insert(
                s2_start + Duration::minutes(i * 4),
                "claude-code",
                "Bash",
                "s2",
                "cargo test --workspace",
                "/proj/b",
            );
        }
        for i in 0..44 {
            insert(
                s2_start + Duration::seconds(i * 30 + 30),
                "claude-code",
                "Edit",
                "s2",
                &format!("src/file{}.rs", i % 3),
                "/proj/b",
            );
        }

        // Noise singleton — drops out under the 5-event / 5-min rule.
        insert(
            now - Duration::hours(3),
            "claude-code",
            "Read",
            "noise",
            "README.md",
            "/proj/d",
        );

        // SESSION 1 — Building. 5 edits on src/lib.rs, 8-minute span,
        // ~2 hours ago. Most recent of the three.
        let s1_start = now - Duration::hours(2);
        for i in 0..5 {
            insert(
                s1_start + Duration::minutes(i * 2),
                "claude-code",
                "Edit",
                "s1",
                "src/lib.rs",
                "/Users/me/proj/fluxmirror",
            );
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
async fn api_sessions_returns_three_clusters() {
    let (_d, state) = sessions_fixture();
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
    let arr = v.as_array().unwrap();
    assert_eq!(arr.len(), 3, "expected 3 sessions, got: {v}");

    // Chronological order: oldest cluster first.
    assert_eq!(arr[0]["lifecycle"], "polishing");
    assert_eq!(arr[1]["lifecycle"], "testing");
    assert_eq!(arr[2]["lifecycle"], "building");
}

#[tokio::test]
async fn api_sessions_names_follow_verb_object_tail_format() {
    let (_d, state) = sessions_fixture();
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
    let v: Value = serde_json::from_slice(&body).unwrap();
    for s in v.as_array().unwrap() {
        let name = s["name"].as_str().expect("name string");
        assert!(
            name.contains(": ") && name.contains('(') && name.contains(')'),
            "name {name} should match `<Verb>: <Object> (<Tail>)`"
        );
    }
}

#[tokio::test]
async fn api_sessions_testing_session_carries_cargo_signature() {
    let (_d, state) = sessions_fixture();
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
    let v: Value = serde_json::from_slice(&body).unwrap();
    let testing = v
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["lifecycle"] == "testing")
        .expect("expected one testing session");
    let name = testing["name"].as_str().unwrap();
    assert!(
        name.starts_with("Tested:"),
        "expected 'Tested:' prefix, got {name}"
    );
    assert!(
        name.contains("cargo cycles"),
        "expected cargo signature in name, got {name}"
    );
}

#[tokio::test]
async fn api_sessions_building_session_targets_single_file() {
    let (_d, state) = sessions_fixture();
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
    let v: Value = serde_json::from_slice(&body).unwrap();
    let building = v
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["lifecycle"] == "building")
        .expect("expected one building session");
    let top_files = building["top_files"].as_array().unwrap();
    assert_eq!(top_files.len(), 1);
    assert_eq!(top_files[0], "src/lib.rs");
    let dominant_cwd = building["dominant_cwd"].as_str().unwrap();
    assert_eq!(dominant_cwd, "/Users/me/proj/fluxmirror");
}

#[tokio::test]
async fn api_sessions_polishing_lifecycle_for_many_files() {
    let (_d, state) = sessions_fixture();
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
    let v: Value = serde_json::from_slice(&body).unwrap();
    let polishing = v
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["lifecycle"] == "polishing")
        .expect("expected one polishing session");
    assert!(polishing["top_files"].as_array().unwrap().len() == 5);
    let name = polishing["name"].as_str().unwrap();
    assert!(name.starts_with("Polished:"), "got {name}");
    assert!(name.contains("Edit-heavy"), "got {name}");
}

#[tokio::test]
async fn api_sessions_drops_singleton_noise() {
    let (_d, state) = sessions_fixture();
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
    let v: Value = serde_json::from_slice(&body).unwrap();
    // Three sessions kept; the singleton README.md Read event is
    // dropped because it has duration < 5 min AND event_count < 5.
    assert_eq!(v.as_array().unwrap().len(), 3);
}

#[tokio::test]
async fn api_sessions_list_omits_per_event_timeline() {
    let (_d, state) = sessions_fixture();
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
    let v: Value = serde_json::from_slice(&body).unwrap();
    for s in v.as_array().unwrap() {
        // The list endpoint elides the events array — verifies we
        // don't ship a heavy payload to the dashboard's first paint.
        assert!(
            s["events"].as_array().map(|a| a.is_empty()).unwrap_or(false),
            "expected empty events array, got {}",
            s["events"]
        );
    }
}

#[tokio::test]
async fn api_sessions_is_deterministic() {
    let (_d, state) = sessions_fixture();
    let app1 = build_router(state.clone());
    let app2 = build_router(state);

    let res1 = app1
        .oneshot(
            Request::builder()
                .uri("/api/sessions")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body1 = to_bytes(res1.into_body(), usize::MAX).await.unwrap();

    let res2 = app2
        .oneshot(
            Request::builder()
                .uri("/api/sessions")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body2 = to_bytes(res2.into_body(), usize::MAX).await.unwrap();
    assert_eq!(body1, body2);
}

#[tokio::test]
async fn api_sessions_400s_on_invalid_from() {
    let (_d, state) = sessions_fixture();
    let app = build_router(state);
    let res = app
        .oneshot(
            Request::builder()
                .uri("/api/sessions?from=not-a-date")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn api_sessions_400s_when_to_not_after_from() {
    let (_d, state) = sessions_fixture();
    let app = build_router(state);
    let res = app
        .oneshot(
            Request::builder()
                .uri("/api/sessions?from=2026-04-26&to=2026-04-26")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}
