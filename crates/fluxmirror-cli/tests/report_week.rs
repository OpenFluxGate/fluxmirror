// Integration test for `fluxmirror week`.
//
// Seeds rows spread across 5 distinct days inside the inclusive 7-day
// window the binary computes, then asserts the rendered report contains
// the title, every fixture agent name, the per-day-totals and
// day-distribution sections, and at least three day rows.

use std::path::PathBuf;
use std::process::Command;

use chrono::{Duration, NaiveDate, Utc};
use chrono_tz::Tz;
use fluxmirror_store::SqliteStore;
use rusqlite::{params, Connection};
use tempfile::TempDir;

fn fixture_db_week_busy() -> (TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.db");
    let _store = SqliteStore::open(&path).unwrap();
    let conn = Connection::open(&path).unwrap();

    let now = Utc::now();
    // Anchor each row at noon of the relative day so we never straddle
    // a UTC midnight boundary between fixture creation and the binary's
    // own week_range() call.
    let day_minus = |days: i64, hour: u32, minute: u32| {
        let d = (now - Duration::days(days)).date_naive();
        d.and_hms_opt(hour, minute, 0)
            .unwrap()
            .and_utc()
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
    };

    // Spread 12 rows across day-1 .. day-5 so at least 5 distinct days
    // have data — covers the daily_totals / day-distribution surfaces.
    let rows: Vec<(String, &str, &str, &str, &str, &str)> = vec![
        (day_minus(1, 12, 0), "claude-code", "Edit", "c1", "src/foo.rs", "/proj/a"),
        (day_minus(1, 12, 5), "claude-code", "Edit", "c1", "src/foo.rs", "/proj/a"),
        (day_minus(1, 12, 10), "claude-code", "Read", "c1", "src/bar.rs", "/proj/a"),
        (day_minus(2, 12, 0), "claude-code", "Bash", "c2", "cargo build", "/proj/a"),
        (day_minus(2, 12, 30), "gemini-cli", "read_file", "g1", "README.md", "/proj/b"),
        (day_minus(3, 12, 0), "qwen-code", "Edit", "q1", "docs/note.md", "/proj/a"),
        (day_minus(3, 12, 30), "qwen-code", "Edit", "q1", "docs/note.md", "/proj/a"),
        (day_minus(4, 12, 0), "claude-code", "Edit", "c3", "src/baz.rs", "/proj/a"),
        (day_minus(4, 12, 5), "claude-code", "Edit", "c3", "src/baz.rs", "/proj/a"),
        (day_minus(5, 12, 0), "gemini-cli", "edit_file", "g2", "src/foo.rs", "/proj/b"),
        (day_minus(5, 12, 30), "gemini-cli", "edit_file", "g2", "src/foo.rs", "/proj/b"),
        (day_minus(5, 12, 45), "gemini-cli", "run_shell_command", "g2", "ls", "/proj/b"),
    ];

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
    (dir, path)
}

fn fluxmirror_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_fluxmirror"))
}

#[test]
fn week_human_lists_agents_per_day_totals_and_distribution() {
    let (_dir, db) = fixture_db_week_busy();

    let output = fluxmirror_bin()
        .args(["week", "--tz", "UTC", "--lang", "english"])
        .arg("--db")
        .arg(&db)
        .arg("--format")
        .arg("human")
        .output()
        .expect("spawn fluxmirror week");

    assert!(
        output.status.success(),
        "non-zero exit: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();

    assert!(stdout.contains("Last 7 Days"), "missing title:\n{stdout}");
    assert!(stdout.contains("Per-day totals"), "missing per-day:\n{stdout}");
    assert!(
        stdout.contains("Day distribution"),
        "missing distribution:\n{stdout}"
    );

    for agent in ["claude-code", "gemini-cli", "qwen-code"] {
        assert!(
            stdout.contains(agent),
            "missing agent {agent} in:\n{stdout}"
        );
    }

    // Per-day-totals table emits 7 rows (one per day in the window).
    // Each row begins with `| YYYY-MM-DD (`. Count those occurrences.
    let day_rows: Vec<&str> = stdout
        .lines()
        .filter(|l| l.starts_with("| 20") && l.contains(") |"))
        .collect();
    assert!(
        day_rows.len() >= 3,
        "expected at least 3 day rows, got {}:\n{stdout}",
        day_rows.len()
    );

    // The day-distribution chart references all 7 inclusive dates.
    let tz: Tz = "UTC".parse().unwrap();
    let today = Utc::now().with_timezone(&tz).date_naive();
    let week_start: NaiveDate = today - Duration::days(6);
    let week_start_str = week_start.format("%Y-%m-%d").to_string();
    let today_str = today.format("%Y-%m-%d").to_string();
    assert!(
        stdout.contains(&week_start_str),
        "missing week start {week_start_str} in:\n{stdout}"
    );
    assert!(
        stdout.contains(&today_str),
        "missing today {today_str} in:\n{stdout}"
    );

    // At least one insight bullet must surface.
    assert!(
        stdout.contains("Days active:"),
        "missing days-active insight:\n{stdout}"
    );
}

#[test]
fn week_korean_translates_section_titles() {
    let (_dir, db) = fixture_db_week_busy();
    let ko = fluxmirror_bin()
        .args(["week", "--tz", "UTC", "--lang", "korean"])
        .arg("--db")
        .arg(&db)
        .output()
        .unwrap();
    assert!(ko.status.success());
    let s = String::from_utf8(ko.stdout).unwrap();
    assert!(s.contains("지난 7일"), "missing ko title:\n{s}");
    assert!(s.contains("일별 합계"), "missing ko per-day heading:\n{s}");
    assert!(s.contains("요일 분포"), "missing ko distribution heading:\n{s}");
}

#[test]
fn week_format_json_is_reserved_but_unimplemented() {
    let (_dir, db) = fixture_db_week_busy();
    let output = fluxmirror_bin()
        .args(["week", "--tz", "UTC", "--lang", "english", "--format", "json"])
        .arg("--db")
        .arg(&db)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("not yet implemented"));
}

#[test]
fn week_empty_db_emits_limited_activity_line() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.db");
    let _store = SqliteStore::open(&path).unwrap();
    let output = fluxmirror_bin()
        .args(["week", "--tz", "UTC", "--lang", "english"])
        .arg("--db")
        .arg(&path)
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("Limited activity this week."),
        "expected limited-activity line in:\n{stdout}"
    );
}
