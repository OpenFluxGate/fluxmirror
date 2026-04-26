// fluxmirror sqlite — run a SQL query against the events DB.
//
// Drops the slash commands' dependency on the system `sqlite3` CLI by
// emitting the same default output (no header, pipe-separated columns,
// NULL renders as the empty string).
//
// SELECTs print rows; non-query statements (INSERT/UPDATE/CREATE/...)
// execute and print nothing on success. Any SQL error is surfaced to
// stderr and exits 1.

use std::path::PathBuf;
use std::process::ExitCode;

use rusqlite::{types::ValueRef, Connection};

use super::util::{err_exit2, open_db_readwrite};

pub fn run(db: PathBuf, sql: String) -> ExitCode {
    let conn = match open_db_readwrite(&db) {
        Ok(c) => c,
        Err(e) => return err_exit2(format!("fluxmirror sqlite: {e}")),
    };
    match execute(&conn, &sql) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("fluxmirror sqlite: {e}");
            ExitCode::from(1)
        }
    }
}

fn execute(conn: &Connection, sql: &str) -> Result<(), String> {
    let mut stmt = conn.prepare(sql).map_err(|e| format!("prepare: {e}"))?;
    let col_count = stmt.column_count();

    if col_count == 0 {
        // Non-query statement (e.g. INSERT). Run it.
        stmt.raw_execute().map_err(|e| format!("execute: {e}"))?;
        return Ok(());
    }

    let mut rows = stmt.raw_query();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    use std::io::Write;
    while let Some(row) = rows.next().map_err(|e| format!("row: {e}"))? {
        let mut buf = String::new();
        for i in 0..col_count {
            if i > 0 {
                buf.push('|');
            }
            let val = row.get_ref(i).map_err(|e| format!("col {i}: {e}"))?;
            buf.push_str(&render_value(val));
        }
        let _ = writeln!(out, "{buf}");
    }
    Ok(())
}

/// Render a SQLite value the way the `sqlite3` CLI does by default
/// (no quoting, NULL → empty string, BLOBs as a hex string).
fn render_value(v: ValueRef<'_>) -> String {
    match v {
        ValueRef::Null => String::new(),
        ValueRef::Integer(i) => i.to_string(),
        ValueRef::Real(f) => {
            // sqlite3 prints whole-number floats without a decimal
            // (e.g. `1.0` shows as `1.0` in `.mode list`, but in
            // default mode it shows `1.0` too — keep chrono-style
            // simplest form here since these reports never round-trip).
            // We mirror the shortest round-trippable form.
            let s = format!("{f}");
            if s.parse::<f64>().is_ok() {
                s
            } else {
                format!("{f:?}")
            }
        }
        ValueRef::Text(b) => String::from_utf8_lossy(b).into_owned(),
        ValueRef::Blob(b) => {
            // sqlite3 default mode prints BLOBs as binary, which is
            // both ugly and dangerous for our pipe-separated layout.
            // Hex is the closest safe analogue.
            let mut s = String::with_capacity(b.len() * 2);
            for byte in b {
                s.push_str(&format!("{byte:02x}"));
            }
            s
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fluxmirror_store::SqliteStore;
    use rusqlite::params;
    use tempfile::TempDir;

    fn fixture_db() -> (TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("events.db");
        let _store = SqliteStore::open(&path).unwrap();
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute(
            "INSERT INTO agent_events \
             (ts, agent, session, tool, tool_canonical, tool_class, detail, \
              cwd, host, user, schema_version, raw_json) \
             VALUES ('2026-04-26T00:00:00Z', 'claude-code', 's', 'Bash', 'Bash', \
                     'Shell', 'echo hi', '/tmp', 'h', 'u', 1, '{}')",
            params![],
        )
        .unwrap();
        (dir, path)
    }

    /// Run a SELECT through `execute()` against an in-test connection
    /// and capture rendered rows by reproducing the same render logic.
    /// We don't go through stdout to keep the test hermetic.
    fn capture(conn: &Connection, sql: &str) -> Vec<String> {
        let mut stmt = conn.prepare(sql).unwrap();
        let cc = stmt.column_count();
        assert!(cc > 0, "test capture: not a query");
        let mut rows = stmt.raw_query();
        let mut out = Vec::new();
        while let Some(row) = rows.next().unwrap() {
            let mut buf = String::new();
            for i in 0..cc {
                if i > 0 {
                    buf.push('|');
                }
                buf.push_str(&render_value(row.get_ref(i).unwrap()));
            }
            out.push(buf);
        }
        out
    }

    #[test]
    fn select_int_text_null() {
        let (_d, path) = fixture_db();
        let conn = open_db_readwrite(&path).unwrap();
        let lines = capture(&conn, "SELECT 1, 'hi', NULL");
        assert_eq!(lines, vec!["1|hi|"]);
    }

    #[test]
    fn select_real_value() {
        let (_d, path) = fixture_db();
        let conn = open_db_readwrite(&path).unwrap();
        let lines = capture(&conn, "SELECT 3.14");
        assert_eq!(lines, vec!["3.14"]);
    }

    #[test]
    fn select_real_table_row() {
        let (_d, path) = fixture_db();
        let conn = open_db_readwrite(&path).unwrap();
        let lines = capture(&conn, "SELECT agent, tool, detail FROM agent_events");
        assert_eq!(lines, vec!["claude-code|Bash|echo hi"]);
    }

    #[test]
    fn execute_non_query_succeeds_silently() {
        let (_d, path) = fixture_db();
        let conn = open_db_readwrite(&path).unwrap();
        // Run a no-op DDL; should succeed and emit nothing.
        execute(&conn, "CREATE TABLE IF NOT EXISTS extra (x INTEGER)").unwrap();
    }

    #[test]
    fn invalid_sql_returns_err() {
        let (_d, path) = fixture_db();
        let conn = open_db_readwrite(&path).unwrap();
        assert!(execute(&conn, "SELECT THIS IS NOT VALID").is_err());
    }
}
