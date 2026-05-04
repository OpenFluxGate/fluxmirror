// Phase 4 M-A4 — cross-day project clustering.
//
// Walks the trailing `days_back` days of heuristic work sessions
// (produced by `report::sessions::collect_sessions`) and folds them
// into "projects". Two sessions belong to the same project when they
// share a `dominant_cwd` AND the gap between the latest event of the
// older one and the earliest event of the newer one is ≤ 5 days, OR
// their `top_files` overlap by Jaccard ≥ 0.4.
//
// Output is heuristic-only — name and arc are deterministic strings
// derived from the cluster shape. The studio API layer calls
// `fluxmirror_ai::synthesise` on top of this to upgrade name+arc to
// LLM-synthesised text. Keeping the LLM step out of `fluxmirror-core`
// avoids a cyclic crate dependency (the AI crate already depends on
// `fluxmirror-core` for `Config` + redact rules).

use std::collections::{BTreeSet, HashMap};

use chrono::{DateTime, Duration, Utc};
use chrono_tz::Tz;
use rusqlite::Connection;

use super::dto::{Project, ProjectSource, ProjectStatus, Session, SessionLifecycle, WindowRange};
use super::sessions::collect_sessions;

/// Maximum gap (latest end → next start) between two sessions that
/// still keeps them in the same project.
const PROJECT_GAP_DAYS: i64 = 5;

/// Minimum Jaccard overlap on top-files needed to merge two clusters
/// even when the cwd doesn't match.
const TOP_FILES_JACCARD_MIN: f64 = 0.4;

/// "Active" status: last session ended within this window.
const ACTIVE_RECENCY_HOURS: i64 = 24;

/// "Paused" upper bound: last session ended within this window but not
/// in the active window.
const PAUSED_RECENCY_DAYS: i64 = 7;

/// FNV-1a 64-bit, matching `sessions::session_id`'s hash.
const FNV_OFFSET: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;

/// Re-derive every session inside the trailing `days_back` days, then
/// fold the sessions into project clusters. Heuristic only — the name
/// and arc fields are filled with deterministic fallbacks; callers in
/// the studio layer overlay LLM-synthesised text on top.
pub fn collect_projects(
    conn: &Connection,
    days_back: i64,
) -> Result<Vec<Project>, String> {
    if days_back <= 0 {
        return Ok(Vec::new());
    }
    let now = Utc::now();
    let start = now - Duration::days(days_back);
    let range = WindowRange {
        start_utc: start,
        end_utc: now + Duration::seconds(1),
        anchor_date: start.date_naive(),
        tz: "UTC".to_string(),
    };
    let tz: Tz = chrono_tz::UTC;
    let sessions = collect_sessions(conn, &tz, range)?;
    Ok(cluster_sessions_into_projects(&sessions, now))
}

/// Public for testability: feed pre-built sessions in and watch the
/// clustering shape come out. `now` is threaded explicitly so the
/// status classifier is deterministic in tests.
pub fn cluster_sessions_into_projects(
    sessions: &[Session],
    now: DateTime<Utc>,
) -> Vec<Project> {
    if sessions.is_empty() {
        return Vec::new();
    }

    // Sort by start ascending so the gap rule is always "compare to the
    // most recent end so far."
    let mut sorted: Vec<&Session> = sessions.iter().collect();
    sorted.sort_by(|a, b| a.start.cmp(&b.start));

    let mut clusters: Vec<Vec<&Session>> = Vec::new();
    for s in sorted {
        let mut placed = false;
        for cluster in clusters.iter_mut() {
            if should_merge(cluster, s) {
                cluster.push(s);
                placed = true;
                break;
            }
        }
        if !placed {
            clusters.push(vec![s]);
        }
    }

    clusters
        .into_iter()
        .map(|c| build_project(&c, now))
        .collect()
}

/// True when `session` belongs in the existing `cluster`. The dominant
/// cwd needs to match at least once AND the gap to the most-recent end
/// is within bounds, OR the top-files Jaccard overlap is high enough on
/// any cluster member.
fn should_merge(cluster: &[&Session], session: &Session) -> bool {
    let s_start = match parse_iso(&session.start) {
        Some(t) => t,
        None => return false,
    };

    // Gap check uses the cluster's latest end.
    let latest_end = cluster
        .iter()
        .filter_map(|s| parse_iso(&s.end))
        .max();
    let gap_ok = match latest_end {
        Some(end) => (s_start - end).num_days().abs() <= PROJECT_GAP_DAYS,
        None => true,
    };

    // Same dominant cwd anywhere in the cluster?
    let cwd_match = if let Some(cwd) = &session.dominant_cwd {
        cluster
            .iter()
            .any(|s| s.dominant_cwd.as_ref() == Some(cwd))
    } else {
        false
    };

    if cwd_match && gap_ok {
        return true;
    }

    // Fallback: high top-files overlap with any cluster member, gap
    // still within bounds.
    if !gap_ok {
        return false;
    }
    cluster.iter().any(|s| {
        jaccard(&s.top_files, &session.top_files) >= TOP_FILES_JACCARD_MIN
    })
}

/// Jaccard index over two file lists. Empty inputs return 0.0 so the
/// merge rule never trips on a quiet session with no top files.
fn jaccard(a: &[String], b: &[String]) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let set_a: BTreeSet<&String> = a.iter().collect();
    let set_b: BTreeSet<&String> = b.iter().collect();
    let inter = set_a.intersection(&set_b).count();
    let union = set_a.union(&set_b).count();
    if union == 0 {
        0.0
    } else {
        inter as f64 / union as f64
    }
}

fn build_project(cluster: &[&Session], now: DateTime<Utc>) -> Project {
    debug_assert!(!cluster.is_empty());

    // Sort cluster chronologically so session_ids comes out ordered.
    let mut sorted: Vec<&Session> = cluster.to_vec();
    sorted.sort_by(|a, b| a.start.cmp(&b.start));

    let session_ids: Vec<String> = sorted.iter().map(|s| s.id.clone()).collect();
    let start = sorted.first().map(|s| s.start.clone()).unwrap_or_default();
    let end = sorted.last().map(|s| s.end.clone()).unwrap_or_default();

    let total_events: i64 = sorted.iter().map(|s| s.event_count).sum();
    // Cost overlay isn't threaded into Session today — leave 0.0 and let
    // the studio layer fill this in if it ever attaches per-session
    // cost numbers.
    let total_usd = 0.0_f64;

    let mut cwd_counts: HashMap<String, u64> = HashMap::new();
    for s in &sorted {
        if let Some(cwd) = &s.dominant_cwd {
            *cwd_counts.entry(cwd.clone()).or_default() += 1;
        }
    }
    let dominant_cwd = cwd_counts
        .into_iter()
        .max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(&a.0)))
        .map(|(k, _)| k);

    let status = classify_status(&sorted, now);

    let id = project_id(&start, &end, dominant_cwd.as_deref());

    let day_count = day_span(&start, &end).max(1);
    let session_count = sorted.len();
    let name = heuristic_name(dominant_cwd.as_deref(), &start, &end);
    let arc = heuristic_arc(session_count, day_count, total_events);

    Project {
        id,
        name,
        arc,
        status,
        session_ids,
        start,
        end,
        total_events,
        total_usd,
        dominant_cwd,
        source: ProjectSource::Heuristic,
    }
}

fn classify_status(sorted: &[&Session], now: DateTime<Utc>) -> ProjectStatus {
    let last = match sorted.last() {
        Some(s) => *s,
        None => return ProjectStatus::Abandoned,
    };
    let last_end = parse_iso(&last.end);
    let recency = last_end.map(|t| now - t);

    let active = recency
        .map(|d| d <= Duration::hours(ACTIVE_RECENCY_HOURS))
        .unwrap_or(false);
    let recent = recency
        .map(|d| d <= Duration::days(PAUSED_RECENCY_DAYS))
        .unwrap_or(false);

    let last_shipping = last.lifecycle == SessionLifecycle::Shipping;
    let any_tag = sorted.iter().any(|s| s.lifecycle == SessionLifecycle::Shipping);

    if last_shipping && any_tag {
        return ProjectStatus::Shipped;
    }
    if active {
        return ProjectStatus::Active;
    }
    if recent {
        return ProjectStatus::Paused;
    }
    ProjectStatus::Abandoned
}

/// Parse the ISO-8601 strings produced by `Session::{start, end}`.
fn parse_iso(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s).ok().map(|dt| dt.with_timezone(&Utc))
}

/// 8-char lowercase hex digest of `start|end|cwd`. Stable across runs.
pub fn project_id(start: &str, end: &str, cwd: Option<&str>) -> String {
    let mut hash = FNV_OFFSET;
    let parts = [start, "|", end, "|", cwd.unwrap_or("")];
    for part in &parts {
        for byte in part.bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(FNV_PRIME);
        }
    }
    format!("{:08x}", (hash >> 32) as u32)
}

/// Heuristic project name: last 3 path segments of the dominant cwd,
/// falling back to a date-stamped placeholder when no cwd resolves.
pub fn heuristic_name(cwd: Option<&str>, start: &str, end: &str) -> String {
    if let Some(cwd) = cwd {
        let parts: Vec<&str> = cwd.split('/').filter(|p| !p.is_empty()).collect();
        if !parts.is_empty() {
            let take = parts.len().min(3);
            let i = parts.len() - take;
            return parts[i..].join("/");
        }
    }
    let s = start.get(..10).unwrap_or(start);
    let e = end.get(..10).unwrap_or(end);
    format!("untitled {s}..{e}")
}

/// Heuristic arc — one short, deterministic sentence.
pub fn heuristic_arc(session_count: usize, day_count: i64, total_events: i64) -> String {
    format!("{session_count} sessions, {day_count} days, {total_events} events.")
}

/// Whole-day count between two ISO timestamps. Negative or unparsable
/// inputs collapse to 1.
fn day_span(start: &str, end: &str) -> i64 {
    let s = parse_iso(start);
    let e = parse_iso(end);
    match (s, e) {
        (Some(a), Some(b)) => {
            let diff = (b - a).num_days();
            diff.max(1)
        }
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::dto::{Session, SessionLifecycle, ToolMixEntry};
    use chrono::TimeZone;

    fn iso(year: i32, mo: u32, day: u32) -> String {
        Utc.with_ymd_and_hms(year, mo, day, 12, 0, 0)
            .unwrap()
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
    }

    fn sess(
        id: &str,
        start: &str,
        end: &str,
        cwd: Option<&str>,
        top_files: &[&str],
        lifecycle: SessionLifecycle,
        events: i64,
    ) -> Session {
        Session {
            id: id.to_string(),
            start: start.to_string(),
            end: end.to_string(),
            agents: vec!["claude-code".into()],
            event_count: events,
            dominant_cwd: cwd.map(String::from),
            top_files: top_files.iter().map(|s| s.to_string()).collect(),
            tool_mix: vec![ToolMixEntry {
                tool: "Edit".into(),
                count: events as u64,
            }],
            lifecycle,
            name: "synthetic".into(),
            intent: None,
            events: vec![],
        }
    }

    #[test]
    fn empty_input_yields_no_projects() {
        let projects = cluster_sessions_into_projects(&[], Utc::now());
        assert!(projects.is_empty());
    }

    #[test]
    fn same_cwd_within_5_day_gap_merges() {
        let a = sess(
            "a1",
            &iso(2026, 4, 1),
            &iso(2026, 4, 1),
            Some("/Users/me/proj/fluxmirror"),
            &["src/lib.rs"],
            SessionLifecycle::Building,
            12,
        );
        let b = sess(
            "b1",
            &iso(2026, 4, 4),
            &iso(2026, 4, 4),
            Some("/Users/me/proj/fluxmirror"),
            &["src/main.rs"],
            SessionLifecycle::Building,
            8,
        );
        let projects =
            cluster_sessions_into_projects(&[a, b], Utc.with_ymd_and_hms(2026, 4, 5, 0, 0, 0).unwrap());
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].session_ids, vec!["a1", "b1"]);
        assert_eq!(projects[0].total_events, 20);
        assert_eq!(
            projects[0].dominant_cwd.as_deref(),
            Some("/Users/me/proj/fluxmirror")
        );
    }

    #[test]
    fn same_cwd_beyond_5_day_gap_splits() {
        let a = sess(
            "a1",
            &iso(2026, 4, 1),
            &iso(2026, 4, 1),
            Some("/proj/x"),
            &[],
            SessionLifecycle::Building,
            10,
        );
        let b = sess(
            "b1",
            &iso(2026, 4, 10),
            &iso(2026, 4, 10),
            Some("/proj/x"),
            &[],
            SessionLifecycle::Building,
            10,
        );
        let projects =
            cluster_sessions_into_projects(&[a, b], Utc.with_ymd_and_hms(2026, 4, 11, 0, 0, 0).unwrap());
        assert_eq!(projects.len(), 2);
    }

    #[test]
    fn jaccard_overlap_merges_across_cwds() {
        let a = sess(
            "a1",
            &iso(2026, 4, 1),
            &iso(2026, 4, 1),
            Some("/proj/old"),
            &["src/lib.rs", "src/main.rs", "Cargo.toml"],
            SessionLifecycle::Building,
            10,
        );
        // Different cwd, but 2/3 top files in common (Jaccard = 2/4 = 0.5).
        let b = sess(
            "b1",
            &iso(2026, 4, 2),
            &iso(2026, 4, 2),
            Some("/proj/new"),
            &["src/lib.rs", "src/main.rs", "README.md"],
            SessionLifecycle::Building,
            10,
        );
        let projects =
            cluster_sessions_into_projects(&[a, b], Utc.with_ymd_and_hms(2026, 4, 3, 0, 0, 0).unwrap());
        assert_eq!(projects.len(), 1, "jaccard 0.5 should merge");
    }

    #[test]
    fn low_jaccard_does_not_merge() {
        let a = sess(
            "a1",
            &iso(2026, 4, 1),
            &iso(2026, 4, 1),
            Some("/proj/old"),
            &["src/lib.rs"],
            SessionLifecycle::Building,
            10,
        );
        let b = sess(
            "b1",
            &iso(2026, 4, 2),
            &iso(2026, 4, 2),
            Some("/proj/new"),
            &["totally/different.md"],
            SessionLifecycle::Building,
            10,
        );
        let projects =
            cluster_sessions_into_projects(&[a, b], Utc.with_ymd_and_hms(2026, 4, 3, 0, 0, 0).unwrap());
        assert_eq!(projects.len(), 2);
    }

    #[test]
    fn status_active_when_last_session_under_24h() {
        let now = Utc.with_ymd_and_hms(2026, 4, 26, 12, 0, 0).unwrap();
        let recent_start = (now - Duration::hours(2))
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let recent_end = (now - Duration::hours(1))
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let s = sess(
            "x",
            &recent_start,
            &recent_end,
            Some("/proj/a"),
            &[],
            SessionLifecycle::Building,
            10,
        );
        let projects = cluster_sessions_into_projects(&[s], now);
        assert_eq!(projects[0].status, ProjectStatus::Active);
    }

    #[test]
    fn status_paused_when_last_session_2_days_ago() {
        let now = Utc.with_ymd_and_hms(2026, 4, 26, 12, 0, 0).unwrap();
        let start = (now - Duration::days(3))
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let end = (now - Duration::days(2))
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let s = sess(
            "x",
            &start,
            &end,
            Some("/proj/a"),
            &[],
            SessionLifecycle::Building,
            10,
        );
        let projects = cluster_sessions_into_projects(&[s], now);
        assert_eq!(projects[0].status, ProjectStatus::Paused);
    }

    #[test]
    fn status_shipped_when_last_session_was_shipping() {
        let now = Utc.with_ymd_and_hms(2026, 4, 26, 12, 0, 0).unwrap();
        let start = (now - Duration::hours(3))
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let end = (now - Duration::hours(2))
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let s = sess(
            "x",
            &start,
            &end,
            Some("/proj/a"),
            &[],
            SessionLifecycle::Shipping,
            10,
        );
        let projects = cluster_sessions_into_projects(&[s], now);
        assert_eq!(projects[0].status, ProjectStatus::Shipped);
    }

    #[test]
    fn status_abandoned_when_no_recent_activity() {
        let now = Utc.with_ymd_and_hms(2026, 4, 26, 12, 0, 0).unwrap();
        let start = (now - Duration::days(20))
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let end = (now - Duration::days(19))
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let s = sess(
            "x",
            &start,
            &end,
            Some("/proj/a"),
            &[],
            SessionLifecycle::Building,
            10,
        );
        let projects = cluster_sessions_into_projects(&[s], now);
        assert_eq!(projects[0].status, ProjectStatus::Abandoned);
    }

    #[test]
    fn project_id_is_deterministic() {
        let a = project_id("2026-04-01T00:00:00Z", "2026-04-05T00:00:00Z", Some("/proj/x"));
        let b = project_id("2026-04-01T00:00:00Z", "2026-04-05T00:00:00Z", Some("/proj/x"));
        assert_eq!(a, b);
        assert_eq!(a.len(), 8);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit() && !c.is_uppercase()));
    }

    #[test]
    fn project_id_changes_with_inputs() {
        let a = project_id("2026-04-01T00:00:00Z", "2026-04-05T00:00:00Z", Some("/proj/x"));
        let b = project_id("2026-04-01T00:00:00Z", "2026-04-05T00:00:00Z", Some("/proj/y"));
        assert_ne!(a, b);
    }

    #[test]
    fn heuristic_name_uses_last_3_path_segments() {
        assert_eq!(
            heuristic_name(Some("/Users/me/proj/fluxmirror"), "2026-04-01T00:00:00Z", "2026-04-02T00:00:00Z"),
            "me/proj/fluxmirror"
        );
        assert_eq!(
            heuristic_name(Some("/proj"), "2026-04-01T00:00:00Z", "2026-04-02T00:00:00Z"),
            "proj"
        );
    }

    #[test]
    fn heuristic_name_fallback_when_no_cwd() {
        let n = heuristic_name(None, "2026-04-01T00:00:00Z", "2026-04-05T00:00:00Z");
        assert!(n.starts_with("untitled"));
        assert!(n.contains("2026-04-01"));
    }

    #[test]
    fn jaccard_handles_empty_inputs() {
        assert_eq!(jaccard(&[], &[]), 0.0);
        assert_eq!(jaccard(&["a".into()], &[]), 0.0);
    }

    #[test]
    fn jaccard_full_overlap() {
        let a = vec!["x".into(), "y".into()];
        let b = vec!["x".into(), "y".into()];
        assert_eq!(jaccard(&a, &b), 1.0);
    }
}
