// Integration test for `fluxmirror compare --format html` (M5.4).
//
// Fixture has rows for both today and yesterday. The compare HTML card
// must render both date labels in the title and emit a Δ column.

use std::path::PathBuf;
use std::process::Command;

use chrono::{Duration, NaiveDateTime, NaiveTime, TimeZone, Utc};
use rusqlite::{params, Connection};
use tempfile::TempDir;

fn fixture_db(rows: &[(&str, &str, &str, &str, &str, &str)]) -> (TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.db");
    let _store = fluxmirror_store::SqliteStore::open(&path).unwrap();
    let conn = Connection::open(&path).unwrap();
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

fn ts_for(date: chrono::NaiveDate, hour: u32, min: u32) -> String {
    let dt = NaiveDateTime::new(
        date,
        NaiveTime::from_hms_opt(hour, min, 0).unwrap(),
    );
    Utc.from_utc_datetime(&dt)
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

#[test]
fn cli_compare_html_shows_both_date_labels_and_delta_column() {
    // Use UTC dates because the binary is invoked with --tz UTC; Local
    // may differ by one day depending on the test host's TZ.
    let today = Utc::now().date_naive();
    let yesterday = today - Duration::days(1);
    let (_dir, db) = fixture_db(&[
        // today rows
        (
            ts_for(today, 10, 0).as_str(),
            "claude-code",
            "Edit",
            "s1",
            "src/foo.rs",
            "/proj/a",
        ),
        (
            ts_for(today, 10, 5).as_str(),
            "claude-code",
            "Edit",
            "s1",
            "src/bar.rs",
            "/proj/a",
        ),
        (
            ts_for(today, 10, 10).as_str(),
            "claude-code",
            "Read",
            "s1",
            "src/baz.rs",
            "/proj/a",
        ),
        (
            ts_for(today, 10, 15).as_str(),
            "claude-code",
            "Bash",
            "s1",
            "cargo test",
            "/proj/a",
        ),
        // yesterday rows (smaller — drives a positive Δ for total)
        (
            ts_for(yesterday, 11, 0).as_str(),
            "claude-code",
            "Edit",
            "s2",
            "src/foo.rs",
            "/proj/a",
        ),
        (
            ts_for(yesterday, 11, 5).as_str(),
            "claude-code",
            "Read",
            "s2",
            "src/bar.rs",
            "/proj/a",
        ),
    ]);

    let output = Command::new(env!("CARGO_BIN_EXE_fluxmirror"))
        .args([
            "compare", "--tz", "UTC", "--lang", "english", "--html", "--out", "-",
        ])
        .arg("--db")
        .arg(&db)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "non-zero exit: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.starts_with("<!DOCTYPE html>"));
    assert!(stdout.contains("</html>"));
    assert!(stdout.contains("Today vs Yesterday"));
    assert!(
        stdout.contains(&today.format("%Y-%m-%d").to_string()),
        "missing today date"
    );
    assert!(
        stdout.contains(&yesterday.format("%Y-%m-%d").to_string()),
        "missing yesterday date"
    );
    // Δ column header must appear.
    assert!(stdout.contains(">\u{0394}<"), "missing Δ column header");
    // Today=4, yesterday=2 → +100% (and the up arrow once it crosses
    // the 50% highlight threshold).
    assert!(stdout.contains("+100%"), "missing +100% delta");
    assert!(stdout.contains("\u{2191}"), "missing up arrow");
}
