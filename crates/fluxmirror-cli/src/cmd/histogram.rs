// fluxmirror histogram — hourly bucket aggregation.
//
// For every event in `agent_events` whose `ts` falls within
// `[start, end)` (and optionally matches `--agent`), increment a
// bucket keyed by the **local** hour-of-day in `--tz`. Output one line
// per non-empty bucket, sorted ascending by hour:
//
//     HH:00 N
//
// Bucketing is done in Rust (chrono) rather than SQL so timezone
// conversion happens through chrono-tz and never relies on whatever
// timezone the SQLite engine thinks it's in.

use std::path::PathBuf;
use std::process::ExitCode;

use chrono::{DateTime, Utc};
use chrono_tz::Tz;

use super::util::{err_exit2, open_db_readonly, parse_iso8601_z, parse_tz};

pub fn run(
    db: PathBuf,
    tz: String,
    start: String,
    end: String,
    agent: Option<String>,
) -> ExitCode {
    let tz = match parse_tz(&tz) {
        Ok(t) => t,
        Err(e) => return err_exit2(format!("fluxmirror histogram: {e}")),
    };
    let start = match parse_iso8601_z(&start) {
        Ok(t) => t,
        Err(e) => return err_exit2(format!("fluxmirror histogram: --start: {e}")),
    };
    let end = match parse_iso8601_z(&end) {
        Ok(t) => t,
        Err(e) => return err_exit2(format!("fluxmirror histogram: --end: {e}")),
    };
    let conn = match open_db_readonly(&db) {
        Ok(c) => c,
        Err(e) => return err_exit2(format!("fluxmirror histogram: {e}")),
    };

    let buckets = match collect_buckets(&conn, tz, start, end, agent.as_deref()) {
        Ok(b) => b,
        Err(e) => return err_exit2(format!("fluxmirror histogram: {e}")),
    };

    print_buckets(&buckets);
    ExitCode::SUCCESS
}

/// Pull rows from `agent_events` and bucket their timestamps by local
/// hour-of-day. The query filters by ISO-8601 `ts` strings: SQLite's
/// lexicographic ordering is correct for our timestamps because they
/// are all RFC 3339 with the same length and trailing `Z`.
fn collect_buckets(
    conn: &rusqlite::Connection,
    tz: Tz,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    agent: Option<&str>,
) -> Result<[u64; 24], String> {
    let start_str = format_iso(&start);
    let end_str = format_iso(&end);

    let mut buckets = [0u64; 24];

    // Two near-identical query paths — keeping them inline avoids a
    // lifetime gymnastics problem where the optional &str would need
    // to outlive the params vector.
    match agent {
        Some(a) => {
            let mut stmt = conn
                .prepare(
                    "SELECT ts FROM agent_events \
                     WHERE ts >= ?1 AND ts < ?2 AND agent = ?3",
                )
                .map_err(|e| format!("prepare: {e}"))?;
            let rows = stmt
                .query_map([&start_str, &end_str, a], |r| r.get::<_, String>(0))
                .map_err(|e| format!("query: {e}"))?;
            tally(rows, tz, &mut buckets)?;
        }
        None => {
            let mut stmt = conn
                .prepare("SELECT ts FROM agent_events WHERE ts >= ?1 AND ts < ?2")
                .map_err(|e| format!("prepare: {e}"))?;
            let rows = stmt
                .query_map([&start_str, &end_str], |r| r.get::<_, String>(0))
                .map_err(|e| format!("query: {e}"))?;
            tally(rows, tz, &mut buckets)?;
        }
    }

    Ok(buckets)
}

/// Drain a row iterator of `ts` strings into the bucket array. Skips
/// rows whose timestamp fails to parse (legacy / hand-edited data).
fn tally<I>(rows: I, tz: Tz, buckets: &mut [u64; 24]) -> Result<(), String>
where
    I: Iterator<Item = rusqlite::Result<String>>,
{
    for ts_res in rows {
        let ts = ts_res.map_err(|e| format!("row: {e}"))?;
        if let Some(dt_utc) = parse_db_ts(&ts) {
            let hour = dt_utc.with_timezone(&tz).hour_of_day();
            buckets[hour as usize] = buckets[hour as usize].saturating_add(1);
        }
    }
    Ok(())
}

fn print_buckets(buckets: &[u64; 24]) {
    use std::io::Write;
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    for (h, n) in buckets.iter().enumerate() {
        if *n > 0 {
            // Match the python format string: HH:00 N
            let _ = writeln!(out, "{:02}:00 {}", h, n);
        }
    }
}

fn format_iso(t: &DateTime<Utc>) -> String {
    t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// Parse a `ts` string out of the DB. The hook writes RFC 3339 with
/// second precision and a trailing `Z`, but legacy rows or hand-edited
/// data may carry fractional seconds, so accept both.
fn parse_db_ts(s: &str) -> Option<DateTime<Utc>> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.with_timezone(&Utc));
    }
    None
}

trait HourOfDay {
    fn hour_of_day(&self) -> u32;
}

impl HourOfDay for chrono::DateTime<Tz> {
    fn hour_of_day(&self) -> u32 {
        use chrono::Timelike;
        self.hour()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fluxmirror_store::SqliteStore;
    use rusqlite::params;
    use tempfile::TempDir;

    fn fixture_db(rows: &[(&str, &str)]) -> (TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("events.db");
        // Use SqliteStore::open to create the v1 schema, then write
        // raw rows via a fresh read-write connection so we can pin
        // the timestamps exactly.
        let _store = SqliteStore::open(&path).unwrap();
        let conn = rusqlite::Connection::open(&path).unwrap();
        for (ts, agent) in rows {
            conn.execute(
                "INSERT INTO agent_events \
                 (ts, agent, session, tool, tool_canonical, tool_class, detail, \
                  cwd, host, user, schema_version, raw_json) \
                 VALUES (?1, ?2, 's', 'Bash', 'Bash', 'Shell', 'echo', '/tmp', \
                         'h', 'u', 1, '{}')",
                params![ts, agent],
            )
            .unwrap();
        }
        (dir, path)
    }

    fn buckets(
        path: &PathBuf,
        tz: &str,
        start: &str,
        end: &str,
        agent: Option<&str>,
    ) -> [u64; 24] {
        let conn = open_db_readonly(path).unwrap();
        let tz = parse_tz(tz).unwrap();
        let s = parse_iso8601_z(start).unwrap();
        let e = parse_iso8601_z(end).unwrap();
        collect_buckets(&conn, tz, s, e, agent).unwrap()
    }

    #[test]
    fn bucketing_three_rows_two_hours_seoul() {
        // 16:00 UTC = 01:00 KST (UTC+9)
        // 05:30 UTC = 14:30 KST
        // 05:45 UTC = 14:45 KST
        let (_d, path) = fixture_db(&[
            ("2026-04-26T16:00:00Z", "claude-code"),
            ("2026-04-26T05:30:00Z", "claude-code"),
            ("2026-04-26T05:45:00Z", "gemini-cli"),
        ]);
        // window large enough to cover all three rows
        let b = buckets(
            &path,
            "Asia/Seoul",
            "2026-04-25T00:00:00Z",
            "2026-04-28T00:00:00Z",
            None,
        );
        assert_eq!(b[1], 1, "01:00 bucket should hold the 16:00 UTC row");
        assert_eq!(b[14], 2, "14:00 bucket should hold the two 05:xx rows");
        // every other bucket empty
        for (h, count) in b.iter().enumerate() {
            if h != 1 && h != 14 {
                assert_eq!(*count, 0, "bucket {h:02} should be empty");
            }
        }
    }

    #[test]
    fn agent_filter_excludes_other_agents() {
        let (_d, path) = fixture_db(&[
            ("2026-04-26T05:30:00Z", "claude-code"),
            ("2026-04-26T05:45:00Z", "gemini-cli"),
        ]);
        let b = buckets(
            &path,
            "Asia/Seoul",
            "2026-04-25T00:00:00Z",
            "2026-04-28T00:00:00Z",
            Some("claude-code"),
        );
        assert_eq!(b[14], 1);
    }

    #[test]
    fn rows_outside_window_are_excluded() {
        let (_d, path) = fixture_db(&[
            ("2026-04-26T05:30:00Z", "claude-code"),
            ("2030-01-01T00:00:00Z", "claude-code"),
        ]);
        let b = buckets(
            &path,
            "UTC",
            "2026-04-26T00:00:00Z",
            "2026-04-27T00:00:00Z",
            None,
        );
        assert_eq!(b.iter().sum::<u64>(), 1);
        assert_eq!(b[5], 1);
    }
}
