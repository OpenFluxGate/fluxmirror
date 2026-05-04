#![allow(dead_code)]
// Shared fixture helpers for the studio API integration tests.
//
// Mirrors the `tempfile`-backed fixture pattern used by the CLI's
// `report_today.rs` so the studio tests stay close to the canonical
// shape of the events database.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use chrono::{Duration, Utc};
use fluxmirror_studio::AppState;
use rusqlite::{params, Connection, OpenFlags};
use tempfile::TempDir;

/// Build a minimal events.db schema. Mirrors
/// `fluxmirror-store::SqliteStore::migrate` so we don't have to drag
/// the full crate into the test path.
pub fn schema(conn: &Connection) {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_meta \
            (version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL);
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
         );
         CREATE TABLE IF NOT EXISTS events ( \
            id INTEGER PRIMARY KEY AUTOINCREMENT, \
            ts_ms INTEGER NOT NULL, \
            direction TEXT NOT NULL CHECK (direction IN ('c2s','s2c')), \
            method TEXT, \
            message_json TEXT NOT NULL, \
            server_name TEXT NOT NULL \
         );",
    )
    .unwrap();
}

/// Build an empty fixture DB. The connection returned is read/write so
/// callers can seed extra rows on demand; the `AppState` is opened
/// read-only against the same path.
pub fn fixture(rows: &[(&str, &str, &str, &str, &str, &str)]) -> (TempDir, AppState) {
    let dir = tempfile::tempdir().unwrap();
    let path: PathBuf = dir.path().join("events.db");
    {
        let conn = Connection::open(&path).unwrap();
        schema(&conn);
        for (ts, agent, tool, session, detail, cwd) in rows {
            conn.execute(
                "INSERT INTO agent_events \
                 (ts, agent, session, tool, tool_canonical, tool_class, detail, \
                  cwd, host, user, schema_version, raw_json) \
                 VALUES (?1, ?2, ?3, ?4, ?4, 'Other', ?5, ?6, 'h', 'u', 1, '{}')",
                params![ts, agent, session, tool, detail, cwd],
            )
            .unwrap();
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

/// Today-anchored fixture: places `count` Edit events at `count`
/// distinct UTC minutes spanning today's window so the today/week
/// query both pick them up regardless of which timezone the studio
/// resolves at request time.
pub fn fixture_today(count: usize) -> (TempDir, AppState) {
    let now = Utc::now();
    let today_noon = now.date_naive().and_hms_opt(12, 0, 0).unwrap().and_utc();
    let mut rows: Vec<(String, String, String, String, String, String)> = Vec::new();
    for i in 0..count {
        let ts = (today_noon - Duration::minutes(i as i64))
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        rows.push((
            ts,
            "claude-code".into(),
            if i % 3 == 0 {
                "Bash".into()
            } else if i % 3 == 1 {
                "Read".into()
            } else {
                "Edit".into()
            },
            "s1".into(),
            format!("src/file{}.rs", i % 5),
            "/proj/a".into(),
        ));
    }
    let owned: Vec<(&str, &str, &str, &str, &str, &str)> = rows
        .iter()
        .map(|(a, b, c, d, e, f)| (a.as_str(), b.as_str(), c.as_str(), d.as_str(), e.as_str(), f.as_str()))
        .collect();
    fixture(&owned)
}
