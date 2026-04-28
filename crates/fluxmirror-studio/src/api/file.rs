//! `/api/file` — per-file provenance timeline.
//!
//! Wraps `fluxmirror_core::report::data::collect_provenance` so the
//! studio frontend can render every touch on a given file path plus
//! the immediate context of events around each touch.
//!
//! Companion route `/api/file/git` shells out to `git log --follow`
//! for an optional commit-history column. Failure modes there
//! (missing git, file not in repo, command timeout) degrade to an
//! empty list rather than an error response — the page renders fine
//! without the git column.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};

use fluxmirror_core::report::data;

use super::error_response;
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/file", get(handler))
        .route("/file/git", get(git_handler))
}

#[derive(Debug, Deserialize)]
struct FileQuery {
    path: Option<String>,
}

async fn handler(State(state): State<AppState>, Query(q): Query<FileQuery>) -> Response {
    let path = match q.path.as_deref() {
        Some(p) if !p.is_empty() => p,
        _ => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "missing required query parameter: path".to_string(),
            );
        }
    };

    let result = {
        let conn = match state.db.lock() {
            Ok(c) => c,
            Err(e) => {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("db mutex poisoned: {e}"),
                );
            }
        };
        data::collect_provenance(&conn, path)
    };
    match result {
        Ok(d) => Json(d).into_response(),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

/// One commit row from `git log --follow` for the requested path.
#[derive(Debug, Clone, Serialize)]
struct GitCommit {
    hash: String,
    ts: String,
    subject: String,
}

/// Hard cap on how long the git subprocess is allowed to run before we
/// give up and return an empty list. Five seconds is plenty for `git
/// log --follow --max-count=10` on any sane repo.
const GIT_TIMEOUT: Duration = Duration::from_secs(5);

async fn git_handler(Query(q): Query<FileQuery>) -> Response {
    let Some(path) = q.path.as_deref().filter(|p| !p.is_empty()) else {
        return error_response(
            StatusCode::BAD_REQUEST,
            "missing required query parameter: path".to_string(),
        );
    };

    let commits = run_git_log(path).unwrap_or_default();
    Json(commits).into_response()
}

/// Spawn `git log --follow --max-count=10` and parse the
/// tab-separated output. Any error path — git missing, file outside
/// the repo, output we can't parse, the 5s timeout — collapses into
/// `Ok(Vec::new())` so the frontend can keep rendering.
fn run_git_log(path: &str) -> Option<Vec<GitCommit>> {
    let mut child = Command::new("git")
        .args([
            "log",
            "--follow",
            "--max-count=10",
            "--format=%H%x09%aI%x09%s",
            "--",
            path,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_status)) => break,
            Ok(None) => {
                if started.elapsed() >= GIT_TIMEOUT {
                    let _ = child.kill();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(_) => return None,
        }
    }

    let output = child.wait_with_output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8(output.stdout).ok()?;
    let mut commits = Vec::new();
    for line in stdout.lines() {
        let mut parts = line.splitn(3, '\t');
        let hash = parts.next()?.to_string();
        let ts = parts.next()?.to_string();
        let subject = parts.next().unwrap_or("").to_string();
        if hash.is_empty() {
            continue;
        }
        commits.push(GitCommit {
            hash,
            ts,
            subject,
        });
    }
    Some(commits)
}

