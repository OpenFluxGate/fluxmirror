// Integration test for `fluxmirror yesterday --format html` (M5.4).
//
// Same shape as `report_today_html`: fixture rows landing in the
// yesterday window, invoke the binary, assert the HTML carries the
// yesterday-flavoured title and the correct date label.

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

fn yesterday_ts(hour: u32, min: u32) -> String {
    let date = Utc::now().date_naive() - Duration::days(1);
    let dt = NaiveDateTime::new(date, NaiveTime::from_hms_opt(hour, min, 0).unwrap());
    let utc = Utc.from_utc_datetime(&dt);
    utc.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

#[test]
fn cli_yesterday_html_uses_yesterday_title_and_date() {
    let (_dir, db) = fixture_db(&[
        (
            yesterday_ts(9, 0).as_str(),
            "claude-code",
            "Edit",
            "s1",
            "src/foo.rs",
            "/proj/a",
        ),
        (
            yesterday_ts(9, 30).as_str(),
            "claude-code",
            "Read",
            "s1",
            "src/bar.rs",
            "/proj/a",
        ),
    ]);

    let output = Command::new(env!("CARGO_BIN_EXE_fluxmirror"))
        .args([
            "yesterday", "--tz", "UTC", "--lang", "english", "--html", "--out", "-",
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
    assert!(stdout.contains("Yesterday"), "missing yesterday title");
    let date_label = (Utc::now().date_naive() - Duration::days(1))
        .format("%Y-%m-%d")
        .to_string();
    assert!(
        stdout.contains(&date_label),
        "missing yesterday's date label: {}",
        date_label
    );
}
