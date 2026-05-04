// Phase 4 M-A3 — heuristic session pipeline keeps working when AI is off.
//
// This integration test verifies that the heuristic clustering +
// naming flow inside `fluxmirror_core::report::sessions` still
// produces well-formed `Session` DTOs when no AI provider is wired
// up. The `intent` field is opt-in (set by the studio's separate
// fluxmirror-ai decorator) so a pure-core build must leave it `None`
// without erroring.

use std::path::PathBuf;

use chrono::{DateTime, Duration, Utc};
use chrono_tz::UTC;
use rusqlite::{params, Connection};

use fluxmirror_core::config::Config;
use fluxmirror_core::report::dto::{Session, SessionLifecycle, WindowRange};
use fluxmirror_core::report::sessions;

/// Spin up a tempdir-backed events.db and seed a single keepable
/// cluster (5 edits across 8 minutes, well past the duration / count
/// floor). Returns the open connection plus the tempdir guard.
fn fixture(now: DateTime<Utc>) -> (tempfile::TempDir, Connection) {
    let dir = tempfile::tempdir().unwrap();
    let path: PathBuf = dir.path().join("events.db");
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_meta \
            (version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL); \
         CREATE TABLE IF NOT EXISTS agent_events ( \
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
    let s_start = now - Duration::hours(2);
    for i in 0..5 {
        let ts = (s_start + Duration::minutes(i * 2))
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        conn.execute(
            "INSERT INTO agent_events \
             (ts, agent, session, tool, tool_canonical, tool_class, detail, \
              cwd, host, user, schema_version, raw_json) \
             VALUES (?1, 'claude-code', 's1', 'Edit', 'Edit', 'Write', \
                     'src/lib.rs', '/proj/x', 'h', 'u', 1, '{}')",
            params![ts],
        )
        .unwrap();
    }
    (dir, conn)
}

#[test]
fn collect_sessions_succeeds_with_provider_off() {
    let mut cfg = Config::default();
    cfg.ai.provider = "off".into();
    // The provider being "off" is the contract here — heuristic flow
    // must run independently of `cfg.ai`. Even reading the field is
    // optional.
    assert_eq!(cfg.ai.provider, "off");

    let now = Utc::now();
    let (_dir, conn) = fixture(now);

    let range = WindowRange {
        start_utc: now - Duration::hours(24),
        end_utc: now + Duration::seconds(1),
        anchor_date: now.date_naive(),
        tz: "UTC".into(),
    };
    let result = sessions::collect_sessions(&conn, &UTC, range);
    let list: Vec<Session> = result.expect("collect_sessions should not fail");
    assert!(
        !list.is_empty(),
        "fixture should yield at least one keepable cluster"
    );
    for s in &list {
        // Heuristic name is mandatory — it powers the studio header
        // even when AI is disabled.
        assert!(!s.name.is_empty(), "name must always be populated");
        // Intent is the AI-only path. With provider="off" it stays None.
        assert!(
            s.intent.is_none(),
            "intent must be None when no AI step ran (got {:?})",
            s.intent
        );
        // Lifecycle still classifies the cluster.
        assert!(matches!(
            s.lifecycle,
            SessionLifecycle::Building
                | SessionLifecycle::Polishing
                | SessionLifecycle::Investigating
                | SessionLifecycle::Testing
                | SessionLifecycle::Shipping
                | SessionLifecycle::Idle,
        ));
    }
}

#[test]
fn session_without_intent_field_still_deserialises() {
    // Older snapshots (pre M-A3) have no `intent` key. `serde(default)`
    // on the DTO must accept that shape so nothing in the studio's
    // history pipeline breaks.
    let json = r#"{
        "id": "abc12345",
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
    let s: Session = serde_json::from_str(json).expect("legacy shape parses");
    assert_eq!(s.id, "abc12345");
    assert!(s.intent.is_none(), "missing key should default to None");
}

#[test]
fn session_with_intent_field_round_trips() {
    let json = r#"{
        "id": "abc12345",
        "start": "2026-04-26T10:00:00Z",
        "end": "2026-04-26T10:30:00Z",
        "agents": ["claude-code"],
        "event_count": 6,
        "dominant_cwd": null,
        "top_files": [],
        "tool_mix": [],
        "lifecycle": "investigating",
        "name": "Investigated: foo (Read-heavy, 2 cwds)",
        "intent": "Tracing flaky cache eviction",
        "events": []
    }"#;
    let s: Session = serde_json::from_str(json).expect("new shape parses");
    assert_eq!(s.intent.as_deref(), Some("Tracing flaky cache eviction"));

    // Re-serialise and confirm the field round-trips with no change.
    let back = serde_json::to_string(&s).unwrap();
    assert!(
        back.contains("\"intent\":\"Tracing flaky cache eviction\""),
        "intent should re-emit verbatim, got: {back}"
    );
}

#[test]
fn session_with_no_intent_omits_key_on_serialise() {
    // `skip_serializing_if = "Option::is_none"` keeps wire shape lean
    // when AI is off — JSON should NOT carry an `"intent": null`.
    let s = Session {
        id: "abc12345".into(),
        start: "2026-04-26T10:00:00Z".into(),
        end: "2026-04-26T10:30:00Z".into(),
        agents: vec!["claude-code".into()],
        event_count: 6,
        dominant_cwd: None,
        top_files: vec![],
        tool_mix: vec![],
        lifecycle: SessionLifecycle::Idle,
        name: "Idle: — (6 events)".into(),
        intent: None,
        events: vec![],
    };
    let json = serde_json::to_string(&s).unwrap();
    assert!(
        !json.contains("intent"),
        "intent key must be elided when None, got: {json}"
    );
}
