// Phase 4 M-A4 — integration coverage of project clustering.
//
// Builds a 14-day fixture in `agent_events`, runs `collect_projects`
// against it, and asserts the cluster shape: same-cwd-within-5-days
// merges, top-files Jaccard merging, and the four status branches.
//
// We seed events through a hand-rolled schema rather than depending
// on `fluxmirror-store` so this test stays in `fluxmirror-core`'s
// dev-deps shape (tempfile only).

use chrono::{DateTime, Duration, Utc};
use fluxmirror_core::report::dto::{ProjectStatus, SessionLifecycle};
use fluxmirror_core::report::projects::{
    cluster_sessions_into_projects, collect_projects,
};
use fluxmirror_core::report::dto::{Session, ToolMixEntry};
use rusqlite::{params, Connection};
use tempfile::TempDir;

fn schema(conn: &Connection) {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS agent_events ( \
            id INTEGER PRIMARY KEY AUTOINCREMENT, \
            ts TEXT NOT NULL, \
            agent TEXT NOT NULL, \
            session TEXT, \
            tool TEXT, \
            tool_canonical TEXT, \
            tool_class TEXT, \
            detail TEXT, \
            cwd TEXT, \
            host TEXT, \
            user TEXT, \
            schema_version INTEGER NOT NULL DEFAULT 1, \
            raw_json TEXT \
         );",
    )
    .unwrap();
}

fn fixture_db() -> (TempDir, Connection) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.db");
    let conn = Connection::open(&path).unwrap();
    schema(&conn);
    (dir, conn)
}

fn insert(
    conn: &Connection,
    ts: DateTime<Utc>,
    tool: &str,
    detail: &str,
    cwd: &str,
) {
    let ts_str = ts.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    conn.execute(
        "INSERT INTO agent_events \
         (ts, agent, session, tool, tool_canonical, tool_class, detail, \
          cwd, host, user, schema_version, raw_json) \
         VALUES (?1, ?2, ?3, ?4, ?4, 'Other', ?5, ?6, 'h', 'u', 1, '{}')",
        params![ts_str, "claude-code", "s", tool, detail, cwd],
    )
    .unwrap();
}

/// Two same-cwd session clusters within a 5-day gap should merge into
/// a single project. Three clusters spread across 14 days where the
/// last two share a cwd will produce two projects.
#[test]
fn collect_projects_merges_same_cwd_within_window() {
    let (_dir, conn) = fixture_db();
    let now = Utc::now();

    // Cluster A — 13 days ago, cwd /proj/old.
    let a_start = now - Duration::days(13);
    for i in 0..6 {
        insert(
            &conn,
            a_start + Duration::minutes(i),
            "Edit",
            "src/old.rs",
            "/proj/old",
        );
    }

    // Cluster B — 5 days ago, cwd /proj/new.
    let b_start = now - Duration::days(5);
    for i in 0..6 {
        insert(
            &conn,
            b_start + Duration::minutes(i),
            "Edit",
            "src/new.rs",
            "/proj/new",
        );
    }

    // Cluster C — 1 day ago, cwd /proj/new (same as B; 4-day gap →
    // merges with B).
    let c_start = now - Duration::days(1);
    for i in 0..6 {
        insert(
            &conn,
            c_start + Duration::minutes(i),
            "Edit",
            "src/new2.rs",
            "/proj/new",
        );
    }

    let projects = collect_projects(&conn, 14).expect("collect");
    assert_eq!(projects.len(), 2, "two clusters expected, got {projects:?}");

    // The merged /proj/new project should bundle both B and C.
    let new_proj = projects
        .iter()
        .find(|p| p.dominant_cwd.as_deref() == Some("/proj/new"))
        .expect("/proj/new project");
    assert_eq!(new_proj.session_ids.len(), 2);

    let old_proj = projects
        .iter()
        .find(|p| p.dominant_cwd.as_deref() == Some("/proj/old"))
        .expect("/proj/old project");
    assert_eq!(old_proj.session_ids.len(), 1);
}

/// Two clusters with different cwds but high top-files overlap should
/// merge via the Jaccard fallback. Build them as in-memory `Session`
/// values so the test exercises the merge rule without depending on
/// session-clustering's noise filter.
#[test]
fn jaccard_overlap_merges_across_cwds() {
    let now = Utc::now();
    let s_iso = |dt: DateTime<Utc>| dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

    let a = Session {
        id: "a".into(),
        start: s_iso(now - Duration::days(4)),
        end: s_iso(now - Duration::days(4) + Duration::minutes(20)),
        agents: vec!["claude-code".into()],
        event_count: 12,
        dominant_cwd: Some("/proj/old".into()),
        top_files: vec!["src/lib.rs".into(), "src/main.rs".into(), "Cargo.toml".into()],
        tool_mix: vec![ToolMixEntry { tool: "Edit".into(), count: 12 }],
        lifecycle: SessionLifecycle::Building,
        name: "synthetic".into(),
        intent: None,
        events: vec![],
    };
    let b = Session {
        id: "b".into(),
        start: s_iso(now - Duration::days(2)),
        end: s_iso(now - Duration::days(2) + Duration::minutes(20)),
        agents: vec!["claude-code".into()],
        event_count: 12,
        dominant_cwd: Some("/proj/new".into()),
        top_files: vec!["src/lib.rs".into(), "src/main.rs".into(), "README.md".into()],
        tool_mix: vec![ToolMixEntry { tool: "Edit".into(), count: 12 }],
        lifecycle: SessionLifecycle::Building,
        name: "synthetic".into(),
        intent: None,
        events: vec![],
    };
    let projects = cluster_sessions_into_projects(&[a, b], now);
    assert_eq!(projects.len(), 1, "high jaccard should merge across cwds");
    assert_eq!(projects[0].session_ids.len(), 2);
}

/// Status classifier: a session ending in the past 24h should produce
/// an Active project. An older shipping session should produce Shipped.
#[test]
fn status_classifier_branches() {
    let now = Utc::now();
    let s_iso = |dt: DateTime<Utc>| dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

    // Active.
    let active = Session {
        id: "active".into(),
        start: s_iso(now - Duration::hours(2)),
        end: s_iso(now - Duration::hours(1)),
        agents: vec!["claude-code".into()],
        event_count: 5,
        dominant_cwd: Some("/proj/active".into()),
        top_files: vec![],
        tool_mix: vec![],
        lifecycle: SessionLifecycle::Building,
        name: "x".into(),
        intent: None,
        events: vec![],
    };
    let projects = cluster_sessions_into_projects(&[active], now);
    assert_eq!(projects[0].status, ProjectStatus::Active);

    // Paused (3 days back).
    let paused = Session {
        id: "paused".into(),
        start: s_iso(now - Duration::days(3)),
        end: s_iso(now - Duration::days(3) + Duration::hours(1)),
        agents: vec!["claude-code".into()],
        event_count: 5,
        dominant_cwd: Some("/proj/p".into()),
        top_files: vec![],
        tool_mix: vec![],
        lifecycle: SessionLifecycle::Building,
        name: "x".into(),
        intent: None,
        events: vec![],
    };
    let projects = cluster_sessions_into_projects(&[paused], now);
    assert_eq!(projects[0].status, ProjectStatus::Paused);

    // Shipped (last session was Shipping).
    let shipped = Session {
        id: "shipped".into(),
        start: s_iso(now - Duration::hours(3)),
        end: s_iso(now - Duration::hours(2)),
        agents: vec!["claude-code".into()],
        event_count: 5,
        dominant_cwd: Some("/proj/s".into()),
        top_files: vec![],
        tool_mix: vec![],
        lifecycle: SessionLifecycle::Shipping,
        name: "x".into(),
        intent: None,
        events: vec![],
    };
    let projects = cluster_sessions_into_projects(&[shipped], now);
    assert_eq!(projects[0].status, ProjectStatus::Shipped);

    // Abandoned (>7 days, never shipped).
    let abandoned = Session {
        id: "abandoned".into(),
        start: s_iso(now - Duration::days(20)),
        end: s_iso(now - Duration::days(20) + Duration::hours(1)),
        agents: vec!["claude-code".into()],
        event_count: 5,
        dominant_cwd: Some("/proj/d".into()),
        top_files: vec![],
        tool_mix: vec![],
        lifecycle: SessionLifecycle::Idle,
        name: "x".into(),
        intent: None,
        events: vec![],
    };
    let projects = cluster_sessions_into_projects(&[abandoned], now);
    assert_eq!(projects[0].status, ProjectStatus::Abandoned);
}

/// `days_back` <= 0 should return an empty list, no SQL run.
#[test]
fn days_back_zero_returns_empty() {
    let (_dir, conn) = fixture_db();
    let projects = collect_projects(&conn, 0).unwrap();
    assert!(projects.is_empty());
}
