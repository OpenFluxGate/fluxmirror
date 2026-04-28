// Integration test for the `--html` auto-out path (M5.4).
//
// When `--html` is given without `--out`, the binary writes the HTML
// to /tmp/fluxmirror-<subcmd>-<timestamp>.html and prints the absolute
// path to stdout. Verifies for `today`, `yesterday`, `compare`, and
// `agent` that:
//   - exit code is 0
//   - stdout has shape `wrote /tmp/fluxmirror-<subcmd>-<...>.html\n`
//   - the file exists, is non-trivially sized, and starts with DOCTYPE.

use std::path::PathBuf;
use std::process::Command;

use chrono::{NaiveDateTime, NaiveTime, TimeZone, Utc};
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

fn today_ts(hour: u32, min: u32) -> String {
    let now = Utc::now().date_naive();
    let dt = NaiveDateTime::new(now, NaiveTime::from_hms_opt(hour, min, 0).unwrap());
    Utc.from_utc_datetime(&dt)
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn assert_auto_out(stdout: &str, subcmd: &str) -> PathBuf {
    let trimmed = stdout.trim_end();
    let prefix = format!("wrote /tmp/fluxmirror-{}-", subcmd);
    assert!(
        trimmed.starts_with(&prefix),
        "expected stdout to start with `{}`, got: {:?}",
        prefix,
        trimmed
    );
    assert!(
        trimmed.ends_with(".html"),
        "expected stdout to end with `.html`, got: {:?}",
        trimmed
    );
    let path_str = trimmed.trim_start_matches("wrote ").to_string();
    let path = PathBuf::from(path_str);
    assert!(path.exists(), "auto-out path does not exist: {}", path.display());
    let body = std::fs::read_to_string(&path).unwrap();
    assert!(
        body.starts_with("<!DOCTYPE html>"),
        "auto-out file missing DOCTYPE: {}",
        path.display()
    );
    assert!(
        body.len() > 1000,
        "auto-out file too small ({} bytes): {}",
        body.len(),
        path.display()
    );
    path
}

#[test]
fn today_auto_out_writes_to_tmp_and_prints_path() {
    let (_d, db) = fixture_db(&[(
        today_ts(10, 0).as_str(),
        "claude-code",
        "Edit",
        "s1",
        "src/foo.rs",
        "/proj/a",
    )]);
    let output = Command::new(env!("CARGO_BIN_EXE_fluxmirror"))
        .args(["today", "--tz", "UTC", "--lang", "english", "--html"])
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
    let path = assert_auto_out(&stdout, "today");
    let _ = std::fs::remove_file(path);
}

#[test]
fn yesterday_auto_out_writes_to_tmp_and_prints_path() {
    let (_d, db) = fixture_db(&[]);
    let output = Command::new(env!("CARGO_BIN_EXE_fluxmirror"))
        .args(["yesterday", "--tz", "UTC", "--lang", "english", "--html"])
        .arg("--db")
        .arg(&db)
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let path = assert_auto_out(&stdout, "yesterday");
    let _ = std::fs::remove_file(path);
}

#[test]
fn compare_auto_out_writes_to_tmp_and_prints_path() {
    let (_d, db) = fixture_db(&[(
        today_ts(10, 0).as_str(),
        "claude-code",
        "Edit",
        "s1",
        "src/foo.rs",
        "/proj/a",
    )]);
    let output = Command::new(env!("CARGO_BIN_EXE_fluxmirror"))
        .args(["compare", "--tz", "UTC", "--lang", "english", "--html"])
        .arg("--db")
        .arg(&db)
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let path = assert_auto_out(&stdout, "compare");
    let _ = std::fs::remove_file(path);
}

#[test]
fn agent_auto_out_writes_to_tmp_and_prints_path() {
    let (_d, db) = fixture_db(&[(
        today_ts(10, 0).as_str(),
        "claude-code",
        "Edit",
        "s1",
        "src/foo.rs",
        "/proj/a",
    )]);
    let output = Command::new(env!("CARGO_BIN_EXE_fluxmirror"))
        .args([
            "agent", "claude-code",
            "--tz", "UTC", "--lang", "english", "--html",
        ])
        .arg("--db")
        .arg(&db)
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let path = assert_auto_out(&stdout, "agent");
    let _ = std::fs::remove_file(path);
}

#[test]
fn week_auto_out_writes_to_tmp_and_prints_path() {
    let (_d, db) = fixture_db(&[]);
    let output = Command::new(env!("CARGO_BIN_EXE_fluxmirror"))
        .args(["week", "--tz", "UTC", "--lang", "english", "--html"])
        .arg("--db")
        .arg(&db)
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let path = assert_auto_out(&stdout, "week");
    let _ = std::fs::remove_file(path);
}
