// SQLite-backed EventStore.
//
// Owns the v1 schema and the additive migration path that upgrades
// pre-Phase-1 databases in place. `SqliteStore::open` is the only
// entry point — it creates the parent dir, opens the connection,
// applies pragmas (WAL + NORMAL synchronous), and runs `migrate()`
// before returning, so callers never see an un-migrated handle.

use std::collections::HashSet;
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use chrono::{SecondsFormat, Utc};
use rusqlite::{params, Connection};

use fluxmirror_core::{AgentEvent, Error, ProxyEvent, Result};

use crate::EventStore;

/// Current on-disk schema version. Bumped when migrations land.
pub const SCHEMA_VERSION: u32 = 1;

/// SQLite-backed implementation of `EventStore`.
///
/// The internal `Connection` is wrapped in a `Mutex` so the store can
/// be `Sync` and shared across threads. The hook subcommand opens the
/// store on every invocation (short-lived process), so contention is
/// not a concern; the proxy lib still uses its own private store and
/// is not affected by this lock.
pub struct SqliteStore {
    conn: Mutex<Connection>,
    schema_version: u32,
}

impl SqliteStore {
    /// Open (or create) the SQLite DB at `path`. Creates parent dirs,
    /// applies pragmas, and runs `migrate()` once before returning.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(Error::Io)?;
        }
        let conn = Connection::open(path).map_err(map_rusqlite)?;
        conn.busy_timeout(Duration::from_secs(5))
            .map_err(map_rusqlite)?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous  = NORMAL;",
        )
        .map_err(map_rusqlite)?;

        let store = SqliteStore {
            conn: Mutex::new(conn),
            schema_version: SCHEMA_VERSION,
        };
        store.migrate()?;
        Ok(store)
    }
}

impl EventStore for SqliteStore {
    fn write_agent_event(&self, e: &AgentEvent) -> Result<()> {
        let conn = self.conn.lock().map_err(|_| poisoned("conn"))?;
        let cwd_str = e.cwd.to_string_lossy().to_string();
        conn.execute(
            "INSERT INTO agent_events \
             (ts, agent, session, tool, tool_canonical, tool_class, detail, cwd, host, user, schema_version, raw_json) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                format_iso8601(&e.ts_utc),
                e.agent.as_str(),
                e.session,
                e.tool_raw,
                e.tool_canonical.as_str(),
                e.tool_class.as_str(),
                e.detail,
                cwd_str,
                e.host,
                e.user,
                e.schema_version,
                e.raw_json,
            ],
        )
        .map_err(map_rusqlite)?;
        Ok(())
    }

    fn write_proxy_event(&self, e: &ProxyEvent) -> Result<()> {
        let conn = self.conn.lock().map_err(|_| poisoned("conn"))?;
        conn.execute(
            "INSERT INTO events (ts_ms, direction, method, message_json, server_name) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                e.ts_ms,
                e.direction.as_str(),
                e.method,
                e.message_json,
                e.server_name,
            ],
        )
        .map_err(map_rusqlite)?;
        Ok(())
    }

    fn schema_version(&self) -> u32 {
        self.schema_version
    }

    fn migrate(&self) -> Result<()> {
        let mut conn = self.conn.lock().map_err(|_| poisoned("conn"))?;
        let tx = conn.transaction().map_err(map_rusqlite)?;

        // Step 1: schema_meta presence dictates the migration path.
        let meta_present = table_exists(&tx, "schema_meta")?;

        if !meta_present {
            // Two cases collapse here:
            //   a) brand-new DB → no tables at all
            //   b) pre-Phase-1 DB → has agent_events with the legacy
            //      column set but no schema_meta marker
            //
            // In both cases the additive ALTER pass below is safe: it
            // creates missing tables / columns / indexes and never
            // touches existing rows.
            create_schema_meta(&tx)?;
            ensure_agent_events_schema(&tx)?;
            ensure_events_schema(&tx)?;
            insert_schema_version(&tx, SCHEMA_VERSION)?;
        } else {
            let current: u32 = tx
                .query_row(
                    "SELECT COALESCE(MAX(version), 0) FROM schema_meta",
                    [],
                    |r| r.get::<_, i64>(0),
                )
                .map_err(map_rusqlite)? as u32;

            if current >= SCHEMA_VERSION {
                // Idempotent re-open. Still ensure tables exist in
                // case the meta row was inserted manually without the
                // companion tables (paranoid but cheap).
                ensure_agent_events_schema(&tx)?;
                ensure_events_schema(&tx)?;
            } else {
                ensure_agent_events_schema(&tx)?;
                ensure_events_schema(&tx)?;
                insert_schema_version(&tx, SCHEMA_VERSION)?;
            }
        }

        tx.commit().map_err(map_rusqlite)?;
        Ok(())
    }
}

// ---------- migration helpers ----------

fn create_schema_meta(tx: &rusqlite::Transaction<'_>) -> Result<()> {
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_meta (
           version    INTEGER PRIMARY KEY,
           applied_at TEXT NOT NULL
         );",
    )
    .map_err(map_rusqlite)?;
    Ok(())
}

fn insert_schema_version(tx: &rusqlite::Transaction<'_>, version: u32) -> Result<()> {
    let now = format_iso8601(&Utc::now());
    tx.execute(
        "INSERT OR IGNORE INTO schema_meta (version, applied_at) VALUES (?1, ?2)",
        params![version, now],
    )
    .map_err(map_rusqlite)?;
    Ok(())
}

/// Either creates `agent_events` from scratch with the full v1 column
/// set, or — on a legacy DB that already has the table — adds any
/// missing columns via `ALTER TABLE ... ADD COLUMN`. Always ensures
/// the two indexes exist.
fn ensure_agent_events_schema(tx: &rusqlite::Transaction<'_>) -> Result<()> {
    let exists = table_exists(tx, "agent_events")?;
    if !exists {
        tx.execute_batch(
            "CREATE TABLE agent_events (
               id INTEGER PRIMARY KEY AUTOINCREMENT,
               ts TEXT NOT NULL,
               agent TEXT NOT NULL,
               session TEXT,
               tool TEXT,
               tool_canonical TEXT,
               tool_class TEXT,
               detail TEXT,
               cwd TEXT,
               host TEXT,
               user TEXT,
               schema_version INTEGER NOT NULL DEFAULT 1,
               raw_json TEXT
             );",
        )
        .map_err(map_rusqlite)?;
    } else {
        let have = column_set(tx, "agent_events")?;
        // Additive list — all v1 additions on top of the legacy schema.
        // Order matters only for readability; SQLite appends at the end.
        let additions: &[(&str, &str)] = &[
            ("tool_canonical", "TEXT"),
            ("tool_class", "TEXT"),
            ("host", "TEXT"),
            ("user", "TEXT"),
            ("schema_version", "INTEGER NOT NULL DEFAULT 1"),
        ];
        for (name, ddl_type) in additions {
            if !have.contains(*name) {
                let sql = format!(
                    "ALTER TABLE agent_events ADD COLUMN {name} {ddl_type}"
                );
                tx.execute(&sql, []).map_err(map_rusqlite)?;
            }
        }
    }

    tx.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_agent_events_ts    ON agent_events(ts);
         CREATE INDEX IF NOT EXISTS idx_agent_events_agent ON agent_events(agent);",
    )
    .map_err(map_rusqlite)?;
    Ok(())
}

/// Mirrors `agent_events`'s pattern for the proxy `events` table.
fn ensure_events_schema(tx: &rusqlite::Transaction<'_>) -> Result<()> {
    let exists = table_exists(tx, "events")?;
    if !exists {
        tx.execute_batch(
            "CREATE TABLE events (
               id INTEGER PRIMARY KEY AUTOINCREMENT,
               ts_ms INTEGER NOT NULL,
               direction TEXT NOT NULL CHECK (direction IN ('c2s', 's2c')),
               method TEXT,
               message_json TEXT NOT NULL,
               server_name TEXT NOT NULL
             );",
        )
        .map_err(map_rusqlite)?;
    }
    tx.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_events_ts ON events(ts_ms);",
    )
    .map_err(map_rusqlite)?;
    Ok(())
}

fn table_exists(tx: &rusqlite::Transaction<'_>, name: &str) -> Result<bool> {
    let count: i64 = tx
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
            params![name],
            |r| r.get(0),
        )
        .map_err(map_rusqlite)?;
    Ok(count > 0)
}

fn column_set(tx: &rusqlite::Transaction<'_>, table: &str) -> Result<HashSet<String>> {
    let sql = format!("PRAGMA table_info({table})");
    let mut stmt = tx.prepare(&sql).map_err(map_rusqlite)?;
    let rows = stmt
        .query_map([], |r| r.get::<_, String>(1))
        .map_err(map_rusqlite)?;
    let mut out = HashSet::new();
    for r in rows {
        out.insert(r.map_err(map_rusqlite)?);
    }
    Ok(out)
}

// ---------- shared utilities ----------

fn format_iso8601(t: &chrono::DateTime<Utc>) -> String {
    // Second precision, trailing Z — matches the legacy hook formatter
    // so reports from before/after this STEP look identical.
    t.to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn map_rusqlite(e: rusqlite::Error) -> Error {
    Error::Config(format!("sqlite: {e}"))
}

fn poisoned(what: &str) -> Error {
    Error::Config(format!("poisoned mutex: {what}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use fluxmirror_core::{AgentId, Direction, ToolClass, ToolKind};
    use rusqlite::Connection;
    use std::collections::HashSet;
    use std::path::PathBuf;

    fn tmp_db(label: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(format!("{label}.db"));
        (dir, path)
    }

    fn raw_columns(conn: &Connection, table: &str) -> HashSet<String> {
        let sql = format!("PRAGMA table_info({table})");
        let mut stmt = conn.prepare(&sql).unwrap();
        let rows = stmt.query_map([], |r| r.get::<_, String>(1)).unwrap();
        rows.map(|r| r.unwrap()).collect()
    }

    fn raw_index_names(conn: &Connection, table: &str) -> HashSet<String> {
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='index' AND tbl_name=?1")
            .unwrap();
        let rows = stmt
            .query_map([table], |r| r.get::<_, String>(0))
            .unwrap();
        rows.map(|r| r.unwrap()).collect()
    }

    fn sample_agent_event() -> AgentEvent {
        AgentEvent {
            ts_utc: Utc.with_ymd_and_hms(2026, 4, 26, 12, 0, 0).unwrap(),
            schema_version: SCHEMA_VERSION,
            agent: AgentId::ClaudeCode,
            session: "sess-abc".to_string(),
            tool_raw: "Bash".to_string(),
            tool_canonical: ToolKind::Bash,
            tool_class: ToolClass::Shell,
            detail: "echo hi".to_string(),
            cwd: PathBuf::from("/tmp"),
            host: "test-host".to_string(),
            user: "tester".to_string(),
            raw_json: r#"{"tool_name":"Bash"}"#.to_string(),
        }
    }

    fn sample_proxy_event() -> ProxyEvent {
        ProxyEvent {
            ts_ms: 1_000,
            direction: Direction::C2S,
            method: Some("tools/call".to_string()),
            message_json: r#"{"jsonrpc":"2.0","id":1,"method":"tools/call"}"#.to_string(),
            server_name: "fs".to_string(),
        }
    }

    #[test]
    fn open_fresh_db_creates_v1_schema() {
        let (_d, path) = tmp_db("fresh");
        let store = SqliteStore::open(&path).unwrap();
        assert_eq!(store.schema_version(), 1);

        let conn = Connection::open(&path).unwrap();

        // schema_meta seeded with version 1.
        let (ver, applied_at): (i64, String) = conn
            .query_row(
                "SELECT version, applied_at FROM schema_meta WHERE version = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(ver, 1);
        assert!(!applied_at.is_empty());

        // agent_events has all 13 columns.
        let cols = raw_columns(&conn, "agent_events");
        for expected in [
            "id",
            "ts",
            "agent",
            "session",
            "tool",
            "tool_canonical",
            "tool_class",
            "detail",
            "cwd",
            "host",
            "user",
            "schema_version",
            "raw_json",
        ] {
            assert!(cols.contains(expected), "missing column: {expected}");
        }
        assert_eq!(cols.len(), 13);

        // events table also exists.
        let evt_cols = raw_columns(&conn, "events");
        for expected in [
            "id",
            "ts_ms",
            "direction",
            "method",
            "message_json",
            "server_name",
        ] {
            assert!(evt_cols.contains(expected), "missing column: {expected}");
        }

        // Both indexes present.
        let idxs = raw_index_names(&conn, "agent_events");
        assert!(idxs.contains("idx_agent_events_ts"));
        assert!(idxs.contains("idx_agent_events_agent"));
        let evt_idxs = raw_index_names(&conn, "events");
        assert!(evt_idxs.contains("idx_events_ts"));
    }

    #[test]
    fn open_legacy_db_migrates_additively() {
        let (_d, path) = tmp_db("legacy");

        // Hand-craft a pre-Phase-1 agent_events table + one row.
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE agent_events (
                   id INTEGER PRIMARY KEY AUTOINCREMENT,
                   ts TEXT NOT NULL,
                   agent TEXT NOT NULL,
                   session TEXT,
                   tool TEXT,
                   detail TEXT,
                   cwd TEXT,
                   raw_json TEXT
                 );",
            )
            .unwrap();
            conn.execute(
                "INSERT INTO agent_events (ts, agent, session, tool, detail, cwd, raw_json) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    "2026-01-01T00:00:00Z",
                    "claude-code",
                    "old",
                    "Bash",
                    "old-row",
                    "/old",
                    "{}"
                ],
            )
            .unwrap();
        }

        // Open via SqliteStore — should ALTER additively, not drop data.
        let store = SqliteStore::open(&path).unwrap();
        assert_eq!(store.schema_version(), 1);

        let conn = Connection::open(&path).unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM agent_events", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1, "legacy row was dropped during migration");

        let cols = raw_columns(&conn, "agent_events");
        for expected in ["tool_canonical", "tool_class", "host", "user", "schema_version"] {
            assert!(cols.contains(expected), "expected new column {expected}");
        }

        // Existing row's new columns are NULL except schema_version which
        // gets the DEFAULT 1.
        let (canon, class, host, user, sv): (
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            i64,
        ) = conn
            .query_row(
                "SELECT tool_canonical, tool_class, host, user, schema_version \
                 FROM agent_events WHERE session='old'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .unwrap();
        assert!(canon.is_none());
        assert!(class.is_none());
        assert!(host.is_none());
        assert!(user.is_none());
        assert_eq!(sv, 1);

        // schema_meta seeded.
        let ver: i64 = conn
            .query_row("SELECT MAX(version) FROM schema_meta", [], |r| r.get(0))
            .unwrap();
        assert_eq!(ver, 1);

        // events table created on the side.
        assert!(table_exists_raw(&conn, "events"));
    }

    #[test]
    fn reopen_is_idempotent() {
        let (_d, path) = tmp_db("idem");
        let _s1 = SqliteStore::open(&path).unwrap();
        let _s2 = SqliteStore::open(&path).unwrap();

        let conn = Connection::open(&path).unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM schema_meta", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1, "schema_meta should not gain duplicate rows");
        let ver: i64 = conn
            .query_row("SELECT MAX(version) FROM schema_meta", [], |r| r.get(0))
            .unwrap();
        assert_eq!(ver, 1);
    }

    #[test]
    fn write_agent_event_round_trip() {
        let (_d, path) = tmp_db("agent-rt");
        let store = SqliteStore::open(&path).unwrap();
        let event = sample_agent_event();
        store.write_agent_event(&event).unwrap();

        let conn = Connection::open(&path).unwrap();
        let (ts, agent, session, tool, canon, class, detail, cwd, host, user, sv, raw):
            (String, String, String, String, String, String, String, String, String, String, i64, String) =
            conn.query_row(
                "SELECT ts, agent, session, tool, tool_canonical, tool_class, detail, cwd, host, user, schema_version, raw_json \
                 FROM agent_events WHERE id = 1",
                [],
                |r| Ok((
                    r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?,
                    r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?, r.get(10)?, r.get(11)?,
                )),
            ).unwrap();

        assert_eq!(ts, "2026-04-26T12:00:00Z");
        assert_eq!(agent, "claude-code");
        assert_eq!(session, "sess-abc");
        assert_eq!(tool, "Bash");
        assert_eq!(canon, "Bash");
        assert_eq!(class, "Shell");
        assert_eq!(detail, "echo hi");
        assert_eq!(cwd, "/tmp");
        assert_eq!(host, "test-host");
        assert_eq!(user, "tester");
        assert_eq!(sv, 1);
        assert_eq!(raw, r#"{"tool_name":"Bash"}"#);
    }

    #[test]
    fn write_proxy_event_round_trip() {
        let (_d, path) = tmp_db("proxy-rt");
        let store = SqliteStore::open(&path).unwrap();
        let event = sample_proxy_event();
        store.write_proxy_event(&event).unwrap();

        let conn = Connection::open(&path).unwrap();
        let (ts_ms, dir, method, msg, server): (i64, String, Option<String>, String, String) =
            conn.query_row(
                "SELECT ts_ms, direction, method, message_json, server_name \
                 FROM events WHERE id = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .unwrap();
        assert_eq!(ts_ms, 1_000);
        assert_eq!(dir, "c2s");
        assert_eq!(method, Some("tools/call".to_string()));
        assert!(msg.contains("tools/call"));
        assert_eq!(server, "fs");
    }

    fn table_exists_raw(conn: &Connection, name: &str) -> bool {
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                params![name],
                |r| r.get(0),
            )
            .unwrap();
        count > 0
    }
}
