// fluxmirror daily-totals — per-day totals across a window.
//
// For each local day in `[start, end)` (in `--tz`), emit:
//
//     YYYY-MM-DD (Day) | calls=N | agents=a,b,c
//
// Days with zero events are still emitted (`calls=0 | agents=-`). The
// final line aggregates the whole window:
//
//     WEEK TOTAL | calls=N | active_days=N
//
// `Day` is the locale-independent English short weekday name (Mon, Tue,
// …), matching Python's `%a` formatter that the legacy slash command
// produced.

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::process::ExitCode;

use chrono::{DateTime, Duration, NaiveDate, Utc};
use chrono_tz::Tz;

use super::util::{err_exit2, open_db_readonly, parse_iso8601_z, parse_tz};

pub fn run(db: PathBuf, tz: String, start: String, end: String) -> ExitCode {
    let tz = match parse_tz(&tz) {
        Ok(t) => t,
        Err(e) => return err_exit2(format!("fluxmirror daily-totals: {e}")),
    };
    let start = match parse_iso8601_z(&start) {
        Ok(t) => t,
        Err(e) => return err_exit2(format!("fluxmirror daily-totals: --start: {e}")),
    };
    let end = match parse_iso8601_z(&end) {
        Ok(t) => t,
        Err(e) => return err_exit2(format!("fluxmirror daily-totals: --end: {e}")),
    };
    let conn = match open_db_readonly(&db) {
        Ok(c) => c,
        Err(e) => return err_exit2(format!("fluxmirror daily-totals: {e}")),
    };

    let report = match collect(&conn, tz, start, end) {
        Ok(r) => r,
        Err(e) => return err_exit2(format!("fluxmirror daily-totals: {e}")),
    };
    print_report(&report);
    ExitCode::SUCCESS
}

#[derive(Debug)]
struct Report {
    /// Ordered list of local dates spanning the window.
    days: Vec<NaiveDate>,
    /// Per-day call counts.
    calls: HashMap<NaiveDate, u64>,
    /// Per-day distinct agent set.
    agents: HashMap<NaiveDate, BTreeSet<String>>,
}

fn collect(
    conn: &rusqlite::Connection,
    tz: Tz,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Result<Report, String> {
    // Build the full day list first so zero-event days are visible.
    let start_date = start.with_timezone(&tz).date_naive();
    let end_date = end.with_timezone(&tz).date_naive();
    let mut days: Vec<NaiveDate> = Vec::new();
    let mut cur = start_date;
    while cur < end_date {
        days.push(cur);
        cur += Duration::days(1);
    }

    let mut calls: HashMap<NaiveDate, u64> = HashMap::new();
    let mut agents: HashMap<NaiveDate, BTreeSet<String>> = HashMap::new();
    for d in &days {
        calls.insert(*d, 0);
        agents.insert(*d, BTreeSet::new());
    }

    let start_str = start.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let end_str = end.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let mut stmt = conn
        .prepare("SELECT ts, agent FROM agent_events WHERE ts >= ?1 AND ts < ?2")
        .map_err(|e| format!("prepare: {e}"))?;
    let rows = stmt
        .query_map([&start_str, &end_str], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })
        .map_err(|e| format!("query: {e}"))?;

    for row in rows {
        let (ts, agent) = row.map_err(|e| format!("row: {e}"))?;
        if let Ok(dt_utc) = DateTime::parse_from_rfc3339(&ts) {
            let local_date = dt_utc.with_timezone(&tz).date_naive();
            if let Some(c) = calls.get_mut(&local_date) {
                *c += 1;
            }
            if let Some(a) = agents.get_mut(&local_date) {
                a.insert(agent);
            }
        }
    }

    Ok(Report {
        days,
        calls,
        agents,
    })
}

fn print_report(r: &Report) {
    use std::io::Write;
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let mut total: u64 = 0;
    let mut active: u64 = 0;
    for d in &r.days {
        let calls = *r.calls.get(d).unwrap_or(&0);
        total += calls;
        if calls > 0 {
            active += 1;
        }
        let agents_set = r.agents.get(d);
        let agents_str = match agents_set {
            Some(set) if !set.is_empty() => {
                set.iter().cloned().collect::<Vec<_>>().join(",")
            }
            _ => "-".to_string(),
        };
        // Match Python's f"{d} ({weekday}) | calls={n} | agents={a}".
        // chrono's %a yields English Mon/Tue/Wed/... regardless of
        // system locale because we never set one.
        let _ = writeln!(
            out,
            "{} ({}) | calls={} | agents={}",
            d.format("%Y-%m-%d"),
            d.format("%a"),
            calls,
            agents_str,
        );
    }
    let _ = writeln!(
        out,
        "WEEK TOTAL | calls={} | active_days={}",
        total, active
    );
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

    fn render(
        path: &PathBuf,
        tz: &str,
        start: &str,
        end: &str,
    ) -> (Vec<String>, String) {
        let conn = open_db_readonly(path).unwrap();
        let tz = parse_tz(tz).unwrap();
        let s = parse_iso8601_z(start).unwrap();
        let e = parse_iso8601_z(end).unwrap();
        let report = collect(&conn, tz, s, e).unwrap();

        let mut total: u64 = 0;
        let mut active: u64 = 0;
        let mut lines: Vec<String> = Vec::new();
        for d in &report.days {
            let calls = *report.calls.get(d).unwrap_or(&0);
            total += calls;
            if calls > 0 {
                active += 1;
            }
            let agents_str = match report.agents.get(d) {
                Some(set) if !set.is_empty() => {
                    set.iter().cloned().collect::<Vec<_>>().join(",")
                }
                _ => "-".to_string(),
            };
            lines.push(format!(
                "{} ({}) | calls={} | agents={}",
                d.format("%Y-%m-%d"),
                d.format("%a"),
                calls,
                agents_str
            ));
        }
        let summary = format!(
            "WEEK TOTAL | calls={} | active_days={}",
            total, active
        );
        (lines, summary)
    }

    #[test]
    fn seven_day_window_with_two_active_days() {
        // Window: 2026-04-20T00:00Z .. 2026-04-27T00:00Z (7 days, UTC)
        // Two events on Apr 22, one on Apr 25.
        let (_d, path) = fixture_db(&[
            ("2026-04-22T10:00:00Z", "claude-code"),
            ("2026-04-22T11:00:00Z", "gemini-cli"),
            ("2026-04-25T00:00:00Z", "claude-code"),
        ]);
        let (lines, summary) = render(
            &path,
            "UTC",
            "2026-04-20T00:00:00Z",
            "2026-04-27T00:00:00Z",
        );
        assert_eq!(lines.len(), 7, "expected 7 day lines");
        assert_eq!(summary, "WEEK TOTAL | calls=3 | active_days=2");
        // Apr 20 had no rows
        assert!(lines[0].starts_with("2026-04-20"));
        assert!(lines[0].contains("calls=0"));
        assert!(lines[0].contains("agents=-"));
        // Apr 22 had 2 rows from 2 distinct agents
        assert!(lines[2].starts_with("2026-04-22"));
        assert!(lines[2].contains("calls=2"));
        assert!(lines[2].contains("agents=claude-code,gemini-cli"));
        // Apr 25 had 1 row
        assert!(lines[5].starts_with("2026-04-25"));
        assert!(lines[5].contains("calls=1"));
        assert!(lines[5].contains("agents=claude-code"));
    }

    #[test]
    fn day_label_uses_english_weekday_abbrev() {
        // 2026-04-22 is a Wednesday.
        let (_d, path) = fixture_db(&[]);
        let (lines, _) = render(
            &path,
            "UTC",
            "2026-04-22T00:00:00Z",
            "2026-04-23T00:00:00Z",
        );
        assert_eq!(lines.len(), 1);
        assert!(lines[0].starts_with("2026-04-22 (Wed)"), "got {:?}", lines[0]);
    }

    #[test]
    fn empty_window_yields_only_summary() {
        let (_d, path) = fixture_db(&[]);
        let (lines, summary) = render(
            &path,
            "UTC",
            "2026-04-22T00:00:00Z",
            "2026-04-22T00:00:00Z",
        );
        assert!(lines.is_empty());
        assert_eq!(summary, "WEEK TOTAL | calls=0 | active_days=0");
    }
}
