// Integration tests for `/api/file?path=…`.
//
// Spins up the same axum router the binary builds, drives it via
// `tower::ServiceExt::oneshot`, and asserts the JSON shape matches
// `fluxmirror_core::report::dto::ProvenanceData`.

mod common;

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use serde_json::Value;
use tower::ServiceExt;

use fluxmirror_studio::build_router;

const FIXTURE_PATH: &str = "src/foo.rs";

fn provenance_fixture() -> (tempfile::TempDir, fluxmirror_studio::AppState) {
    common::fixture(&[
        // Edits on the tracked path, plus surrounding noise.
        (
            "2026-04-26T10:00:00Z",
            "claude-code",
            "Edit",
            "s1",
            FIXTURE_PATH,
            "/proj/a",
        ),
        (
            "2026-04-26T10:01:30Z",
            "claude-code",
            "Bash",
            "s1",
            "cargo test --package fluxmirror-core",
            "/proj/a",
        ),
        (
            "2026-04-26T10:02:00Z",
            "claude-code",
            "Read",
            "s1",
            "src/lib.rs",
            "/proj/a",
        ),
        (
            "2026-04-26T10:03:30Z",
            "gemini-cli",
            "edit_file",
            "g1",
            FIXTURE_PATH,
            "/proj/a",
        ),
        // Far-future event — must not appear as context for either touch.
        (
            "2026-04-26T11:30:00Z",
            "claude-code",
            "Read",
            "s1",
            "src/other.rs",
            "/proj/a",
        ),
        // Touch on a different path — must not show up in our query.
        (
            "2026-04-26T11:31:00Z",
            "claude-code",
            "Edit",
            "s1",
            "src/other.rs",
            "/proj/a",
        ),
    ])
}

#[tokio::test]
async fn api_file_returns_provenance_for_known_path() {
    let (_dir, state) = provenance_fixture();
    let app = build_router(state);

    let uri = format!(
        "/api/file?path={}",
        urlencoding(&FIXTURE_PATH.to_string())
    );
    let res = app
        .oneshot(Request::builder().uri(&uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let v: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["path"], FIXTURE_PATH);
    assert_eq!(v["total_touches"].as_i64().unwrap(), 2);

    let agents = v["agents"].as_array().unwrap();
    assert_eq!(agents.len(), 2);
    let names: Vec<&str> = agents.iter().map(|a| a["agent"].as_str().unwrap()).collect();
    assert!(names.contains(&"claude-code"));
    assert!(names.contains(&"gemini-cli"));

    let events = v["events"].as_array().unwrap();
    assert_eq!(events.len(), 2);
    // Chronological order.
    assert_eq!(events[0]["agent"], "claude-code");
    assert_eq!(events[1]["agent"], "gemini-cli");

    // Both touches should carry the canonical tool name.
    assert_eq!(events[0]["tool"], "Edit");
    assert_eq!(events[1]["tool"], "edit_file");
}

#[tokio::test]
async fn api_file_picks_correct_before_after_context() {
    let (_dir, state) = provenance_fixture();
    let app = build_router(state);

    let uri = format!(
        "/api/file?path={}",
        urlencoding(&FIXTURE_PATH.to_string())
    );
    let res = app
        .oneshot(Request::builder().uri(&uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let v: Value = serde_json::from_slice(&body).unwrap();

    // First touch (10:00:00) — no events strictly before in window,
    // three after: 10:01:30 Bash, 10:02:00 Read, 10:03:30 edit_file.
    let first = &v["events"][0];
    assert!(
        first["before_context"].as_array().unwrap().is_empty(),
        "expected empty before_context for first touch, got {:?}",
        first["before_context"]
    );
    let after = first["after_context"].as_array().unwrap();
    assert_eq!(after.len(), 3);
    assert_eq!(after[0]["tool"], "Bash");
    assert_eq!(after[1]["tool"], "Read");
    assert_eq!(after[2]["tool"], "edit_file");

    // Second touch (10:03:30) — three before in window, none after
    // (next event is 11:30 which is well outside ±5 min).
    let second = &v["events"][1];
    let before = second["before_context"].as_array().unwrap();
    assert_eq!(before.len(), 3);
    // chronological order in before_context.
    assert_eq!(before[0]["tool"], "Edit");
    assert_eq!(before[1]["tool"], "Bash");
    assert_eq!(before[2]["tool"], "Read");
    assert!(
        second["after_context"].as_array().unwrap().is_empty(),
        "expected empty after_context for last in-window touch, got {:?}",
        second["after_context"]
    );
}

#[tokio::test]
async fn api_file_returns_empty_for_unknown_path() {
    let (_dir, state) = provenance_fixture();
    let app = build_router(state);

    let res = app
        .oneshot(
            Request::builder()
                .uri("/api/file?path=does/not/exist.rs")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let v: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["path"], "does/not/exist.rs");
    assert_eq!(v["total_touches"].as_i64().unwrap(), 0);
    assert!(v["agents"].as_array().unwrap().is_empty());
    assert!(v["events"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn api_file_400s_on_missing_path_param() {
    let (_dir, state) = provenance_fixture();
    let app = build_router(state);

    let res = app
        .oneshot(
            Request::builder()
                .uri("/api/file")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn api_file_400s_on_empty_path_value() {
    let (_dir, state) = provenance_fixture();
    let app = build_router(state);

    let res = app
        .oneshot(
            Request::builder()
                .uri("/api/file?path=")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn api_file_groups_per_agent_counts() {
    let (_dir, state) = common::fixture(&[
        (
            "2026-04-26T01:00:00Z",
            "claude-code",
            "Edit",
            "s1",
            "src/foo.rs",
            "/p",
        ),
        (
            "2026-04-26T02:00:00Z",
            "claude-code",
            "Edit",
            "s1",
            "src/foo.rs",
            "/p",
        ),
        (
            "2026-04-26T03:00:00Z",
            "gemini-cli",
            "edit_file",
            "g1",
            "src/foo.rs",
            "/p",
        ),
    ]);
    let app = build_router(state);

    let res = app
        .oneshot(
            Request::builder()
                .uri("/api/file?path=src%2Ffoo.rs")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let v: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["total_touches"].as_i64().unwrap(), 3);

    let agents = v["agents"].as_array().unwrap();
    assert_eq!(agents.len(), 2);
    // Sorted desc by count: claude-code (2) before gemini-cli (1).
    assert_eq!(agents[0]["agent"], "claude-code");
    assert_eq!(agents[0]["count"].as_i64().unwrap(), 2);
    assert_eq!(agents[1]["agent"], "gemini-cli");
    assert_eq!(agents[1]["count"].as_i64().unwrap(), 1);
}

/// Lightweight URL-encoder for the test paths — pulls in only what we
/// need so we don't add a dev dep just for two tests.
fn urlencoding(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}
