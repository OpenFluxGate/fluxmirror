// fluxmirror per-day-files — per-day new vs edited file counts.
//
// For each local day in `[start, end)` that has any write activity,
// emit one line:
//
//     YYYY-MM-DD | new_files=N | edited_files=N
//
// Days with no writes are skipped entirely. Counts are unique-per-day
// (the same file may appear in both `new_files` on one day and
// `edited_files` on another — that's intentional, the counts are
// scoped to the local day).
//
// Tool routing matches the legacy slash command:
//   - new_files    ←  Write, write_file
//   - edited_files ←  Edit, MultiEdit, edit_file, replace
//
// Hard-coding the raw tool names keeps the report working against
// pre-Phase-1 rows that lack `tool_canonical` / `tool_class`.

use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use std::process::ExitCode;

use chrono::{DateTime, NaiveDate, Utc};
use chrono_tz::Tz;

use super::util::{err_exit2, open_db_readonly, parse_iso8601_z, parse_tz};

const NEW_FILE_TOOLS: &[&str] = &["Write", "write_file"];
const EDIT_FILE_TOOLS: &[&str] = &["Edit", "MultiEdit", "edit_file", "replace"];

pub fn run(db: PathBuf, tz: String, start: String, end: String) -> ExitCode {
    let tz = match parse_tz(&tz) {
        Ok(t) => t,
        Err(e) => return err_exit2(format!("fluxmirror per-day-files: {e}")),
    };
    let start = match parse_iso8601_z(&start) {
        Ok(t) => t,
        Err(e) => return err_exit2(format!("fluxmirror per-day-files: --start: {e}")),
    };
    let end = match parse_iso8601_z(&end) {
        Ok(t) => t,
        Err(e) => return err_exit2(format!("fluxmirror per-day-files: --end: {e}")),
    };
    let conn = match open_db_readonly(&db) {
        Ok(c) => c,
        Err(e) => return err_exit2(format!("fluxmirror per-day-files: {e}")),
    };

    let lines = match collect(&conn, tz, start, end) {
        Ok(l) => l,
        Err(e) => return err_exit2(format!("fluxmirror per-day-files: {e}")),
    };
    use std::io::Write;
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    for line in lines {
        let _ = writeln!(out, "{line}");
    }
    ExitCode::SUCCESS
}

fn collect(
    conn: &rusqlite::Connection,
    tz: Tz,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Result<Vec<String>, String> {
    let start_str = start.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let end_str = end.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

    // Prepared statement scoped to write-class rows only — keeps the
    // scan cheap on large DBs without depending on tool_class being
    // populated.
    let in_clause = NEW_FILE_TOOLS
        .iter()
        .chain(EDIT_FILE_TOOLS.iter())
        .map(|s| format!("'{s}'"))
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "SELECT ts, tool, detail FROM agent_events \
         WHERE ts >= ?1 AND ts < ?2 AND tool IN ({in_clause}) \
                              AND detail IS NOT NULL AND detail != ''"
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| format!("prepare: {e}"))?;
    let rows = stmt
        .query_map([&start_str, &end_str], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .map_err(|e| format!("query: {e}"))?;

    // Per-day unique sets keyed by local date. BTreeMap so iteration
    // is sorted ascending without a separate sort pass.
    let mut writes: BTreeMap<NaiveDate, HashSet<String>> = BTreeMap::new();
    let mut edits: BTreeMap<NaiveDate, HashSet<String>> = BTreeMap::new();

    for row in rows {
        let (ts, tool, detail) = row.map_err(|e| format!("row: {e}"))?;
        let dt_utc = match DateTime::parse_from_rfc3339(&ts) {
            Ok(d) => d.with_timezone(&Utc),
            Err(_) => continue,
        };
        let local_date = dt_utc.with_timezone(&tz).date_naive();
        if NEW_FILE_TOOLS.contains(&tool.as_str()) {
            writes.entry(local_date).or_default().insert(detail);
        } else if EDIT_FILE_TOOLS.contains(&tool.as_str()) {
            edits.entry(local_date).or_default().insert(detail);
        }
    }

    // Union of dates that had any activity.
    let mut all_dates: BTreeMap<NaiveDate, ()> = BTreeMap::new();
    for d in writes.keys().chain(edits.keys()) {
        all_dates.insert(*d, ());
    }

    let mut lines: Vec<String> = Vec::new();
    for d in all_dates.keys() {
        let n = writes.get(d).map(|s| s.len()).unwrap_or(0);
        let e = edits.get(d).map(|s| s.len()).unwrap_or(0);
        if n == 0 && e == 0 {
            continue;
        }
        lines.push(format!(
            "{} | new_files={} | edited_files={}",
            d.format("%Y-%m-%d"),
            n,
            e
        ));
    }
    Ok(lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fluxmirror_store::SqliteStore;
    use rusqlite::params;
    use tempfile::TempDir;

    fn fixture_db(rows: &[(&str, &str, &str)]) -> (TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("events.db");
        let _store = SqliteStore::open(&path).unwrap();
        let conn = rusqlite::Connection::open(&path).unwrap();
        for (ts, tool, detail) in rows {
            conn.execute(
                "INSERT INTO agent_events \
                 (ts, agent, session, tool, tool_canonical, tool_class, detail, \
                  cwd, host, user, schema_version, raw_json) \
                 VALUES (?1, 'claude-code', 's', ?2, ?2, 'Write', ?3, '/tmp', \
                         'h', 'u', 1, '{}')",
                params![ts, tool, detail],
            )
            .unwrap();
        }
        (dir, path)
    }

    fn render(path: &PathBuf, tz: &str, start: &str, end: &str) -> Vec<String> {
        let conn = open_db_readonly(path).unwrap();
        let tz = parse_tz(tz).unwrap();
        let s = parse_iso8601_z(start).unwrap();
        let e = parse_iso8601_z(end).unwrap();
        collect(&conn, tz, s, e).unwrap()
    }

    #[test]
    fn day_a_writes_and_edits_day_b_edits_only() {
        // Day A (UTC 2026-04-22): 1 Write + 2 Edit
        // Day B (UTC 2026-04-23): 1 Edit
        let (_d, path) = fixture_db(&[
            ("2026-04-22T01:00:00Z", "Write", "/tmp/new.txt"),
            ("2026-04-22T02:00:00Z", "Edit", "/tmp/a.txt"),
            ("2026-04-22T03:00:00Z", "Edit", "/tmp/b.txt"),
            ("2026-04-23T01:00:00Z", "Edit", "/tmp/c.txt"),
        ]);
        let lines = render(
            &path,
            "UTC",
            "2026-04-22T00:00:00Z",
            "2026-04-24T00:00:00Z",
        );
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "2026-04-22 | new_files=1 | edited_files=2");
        assert_eq!(lines[1], "2026-04-23 | new_files=0 | edited_files=1");
    }

    #[test]
    fn unique_per_day_collapses_repeat_edits() {
        // Same file edited 3 times on day A — should count as 1.
        let (_d, path) = fixture_db(&[
            ("2026-04-22T01:00:00Z", "Edit", "/tmp/a.txt"),
            ("2026-04-22T02:00:00Z", "Edit", "/tmp/a.txt"),
            ("2026-04-22T03:00:00Z", "Edit", "/tmp/a.txt"),
        ]);
        let lines = render(
            &path,
            "UTC",
            "2026-04-22T00:00:00Z",
            "2026-04-23T00:00:00Z",
        );
        assert_eq!(lines, vec!["2026-04-22 | new_files=0 | edited_files=1"]);
    }

    #[test]
    fn gemini_snake_case_tools_recognized() {
        let (_d, path) = fixture_db(&[
            ("2026-04-22T01:00:00Z", "write_file", "/tmp/n.txt"),
            ("2026-04-22T02:00:00Z", "edit_file", "/tmp/e.txt"),
            ("2026-04-22T03:00:00Z", "replace", "/tmp/r.txt"),
        ]);
        let lines = render(
            &path,
            "UTC",
            "2026-04-22T00:00:00Z",
            "2026-04-23T00:00:00Z",
        );
        assert_eq!(lines, vec!["2026-04-22 | new_files=1 | edited_files=2"]);
    }

    #[test]
    fn days_with_no_writes_are_skipped() {
        // Read row should not appear; window is wider than write activity.
        let (_d, path) = fixture_db(&[
            ("2026-04-22T01:00:00Z", "Write", "/tmp/x.txt"),
            // not a write/edit tool — must not surface as a row
            ("2026-04-23T01:00:00Z", "Read", "/tmp/y.txt"),
        ]);
        let lines = render(
            &path,
            "UTC",
            "2026-04-22T00:00:00Z",
            "2026-04-24T00:00:00Z",
        );
        assert_eq!(lines, vec!["2026-04-22 | new_files=1 | edited_files=0"]);
    }
}
