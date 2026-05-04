//! `/api/projects` — Phase 4 M-A4.
//!
//! Walks the trailing `?days_back=` (default 30) days of sessions,
//! clusters them into projects via `fluxmirror_core::report::projects`,
//! and overlays an LLM-synthesised name + arc paragraph using the
//! Sonnet-tier project model.
//!
//! The endpoint is heavy (one LLM call per cluster on a cold path), so
//! the entire response is cached in-process for 24h. The cache key is
//! `(days_back, db_path)` so two studios running side-by-side against
//! different DBs don't pollute each other's view.
//!
//! When `Config.ai.provider == "off"`, when the API key is missing, or
//! when the daily budget is exhausted, every cluster keeps its
//! heuristic name + arc + `source: heuristic` and the response is
//! served as-is.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration as StdDuration, Instant};

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::Deserialize;

use fluxmirror_ai::{synthesise, AiError, SynthOptions};
use fluxmirror_core::report::dto::{Project, ProjectSource};
use fluxmirror_core::report::projects;
use fluxmirror_core::Config;
use fluxmirror_store::SqliteStore;

use super::error_response;
use crate::AppState;

/// In-memory response TTL. The arc paragraph rarely changes inside one
/// day; the cap keeps a Studio reload from re-paying the LLM tax.
const CACHE_TTL: StdDuration = StdDuration::from_secs(60 * 60 * 24);

/// Default trailing window when the caller omits `?days_back=`.
const DEFAULT_DAYS_BACK: i64 = 30;

/// Hard ceiling so a typo'd query string can't run away.
const MAX_DAYS_BACK: i64 = 365;

/// Process-local cache. A `Mutex` is fine — handlers are short and the
/// studio is single-user. We don't depend on `axum`'s state for this
/// because the cache lifetime is independent of any request.
static RESPONSE_CACHE: Mutex<Option<CacheEntry>> = Mutex::new(None);

struct CacheEntry {
    days_back: i64,
    db_path: PathBuf,
    inserted: Instant,
    body: Vec<Project>,
}

pub fn router() -> Router<AppState> {
    Router::new().route("/projects", get(handler))
}

#[derive(Debug, Deserialize)]
struct ProjectsQuery {
    days_back: Option<i64>,
}

async fn handler(
    State(state): State<AppState>,
    Query(q): Query<ProjectsQuery>,
) -> Response {
    let days_back = match q.days_back.unwrap_or(DEFAULT_DAYS_BACK) {
        n if n <= 0 => {
            return error_response(
                StatusCode::BAD_REQUEST,
                format!("days_back must be > 0 (got {n})"),
            );
        }
        n if n > MAX_DAYS_BACK => MAX_DAYS_BACK,
        n => n,
    };

    if let Some(hit) = cache_lookup(&state.db_path, days_back) {
        return Json(hit).into_response();
    }

    // Heuristic clustering reads agent_events through the read-only
    // connection threaded through AppState.
    let projects = {
        let conn = match state.db.lock() {
            Ok(c) => c,
            Err(e) => {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("db mutex poisoned: {e}"),
                );
            }
        };
        match projects::collect_projects(&conn, days_back) {
            Ok(p) => p,
            Err(e) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, e),
        }
    };

    // LLM upgrade — best-effort. AI errors are silently swallowed so the
    // endpoint never fails just because Anthropic rate-limited us.
    let cfg = Config::load().unwrap_or_default();
    let upgraded = upgrade_projects_with_llm(&state.db_path, &cfg, projects);

    cache_insert(&state.db_path, days_back, upgraded.clone());
    Json(upgraded).into_response()
}

/// Walk every cluster and upgrade `name`+`arc` with `synthesise()`.
/// On failure (provider off, API error, JSON parse error) the cluster
/// keeps its heuristic name+arc and `source` stays `Heuristic`.
fn upgrade_projects_with_llm(
    db_path: &PathBuf,
    config: &Config,
    mut projects: Vec<Project>,
) -> Vec<Project> {
    if config.ai.provider == "off" {
        return projects;
    }
    // Open a writable handle just for the AI cache. The studio's normal
    // connection is read-only, but the AI layer needs to write
    // `ai_cache` rows on a miss.
    let store = match SqliteStore::open(db_path) {
        Ok(s) => s,
        Err(_) => return projects,
    };

    for project in projects.iter_mut() {
        let opts = SynthOptions::for_project_model(config);
        let ctx = serde_json::json!({
            "sessions_json": serde_json::to_string(&project.session_ids).unwrap_or_else(|_| "[]".into()),
            "date_range": format!("{} → {}", project.start, project.end),
            "dominant_cwds": project.dominant_cwd.clone().unwrap_or_default(),
        });
        let resp = match synthesise(&store, config, "project", &ctx, opts) {
            Ok(r) => r,
            Err(AiError::ProviderNotImplemented) => continue,
            Err(_) => continue,
        };
        match parse_project_json(&resp.text) {
            Some((name, arc)) => {
                project.name = name;
                project.arc = arc;
                project.source = ProjectSource::Llm;
            }
            None => {
                // JSON parse failure → keep heuristic. No panic.
                continue;
            }
        }
    }
    projects
}

/// Pull `{"name": "...", "arc": "..."}` out of an LLM completion. The
/// model is told to return strict JSON; this also strips a single
/// markdown fence in case the model still wraps the JSON in one.
fn parse_project_json(text: &str) -> Option<(String, String)> {
    let trimmed = text.trim();
    let candidate = strip_fence(trimmed);
    let v: serde_json::Value = serde_json::from_str(candidate).ok()?;
    let name = v.get("name").and_then(|s| s.as_str())?.trim().to_string();
    let arc = v.get("arc").and_then(|s| s.as_str())?.trim().to_string();
    if name.is_empty() || arc.is_empty() {
        return None;
    }
    Some((name, arc))
}

/// Drop a leading `\`\`\`json` (and matching trailing ```) if present.
/// Defensive — the prompt explicitly forbids fences but models drift.
fn strip_fence(s: &str) -> &str {
    let s = s.trim();
    let stripped = s
        .strip_prefix("```json")
        .or_else(|| s.strip_prefix("```JSON"))
        .or_else(|| s.strip_prefix("```"));
    let s = match stripped {
        Some(rest) => rest.trim_start_matches('\n'),
        None => s,
    };
    s.strip_suffix("```").unwrap_or(s).trim_end()
}

fn cache_lookup(db_path: &PathBuf, days_back: i64) -> Option<Vec<Project>> {
    let guard = RESPONSE_CACHE.lock().ok()?;
    let entry = guard.as_ref()?;
    if entry.days_back != days_back {
        return None;
    }
    if &entry.db_path != db_path {
        return None;
    }
    if entry.inserted.elapsed() > CACHE_TTL {
        return None;
    }
    Some(entry.body.clone())
}

fn cache_insert(db_path: &PathBuf, days_back: i64, body: Vec<Project>) {
    if let Ok(mut guard) = RESPONSE_CACHE.lock() {
        *guard = Some(CacheEntry {
            days_back,
            db_path: db_path.clone(),
            inserted: Instant::now(),
            body,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_project_json_happy_path() {
        let body = r#"{"name": "Phase 4 M-A4", "arc": "Three days of clustering work."}"#;
        let (n, a) = parse_project_json(body).unwrap();
        assert_eq!(n, "Phase 4 M-A4");
        assert!(a.starts_with("Three"));
    }

    #[test]
    fn parse_project_json_strips_fence() {
        let body = "```json\n{\"name\":\"x\",\"arc\":\"y\"}\n```";
        let (n, a) = parse_project_json(body).unwrap();
        assert_eq!(n, "x");
        assert_eq!(a, "y");
    }

    #[test]
    fn parse_project_json_rejects_missing_keys() {
        assert!(parse_project_json("{\"name\":\"x\"}").is_none());
        assert!(parse_project_json("{\"arc\":\"y\"}").is_none());
    }

    #[test]
    fn parse_project_json_rejects_empty_strings() {
        assert!(parse_project_json("{\"name\":\"\",\"arc\":\"y\"}").is_none());
    }

    #[test]
    fn parse_project_json_rejects_non_object() {
        assert!(parse_project_json("not json").is_none());
        assert!(parse_project_json("[1, 2, 3]").is_none());
    }
}
