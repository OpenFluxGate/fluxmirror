// Integration test for `fluxmirror today --format html` (M5.4).
//
// Builds a fixture SQLite DB with a few rows landing in the today
// window, invokes the binary, and asserts the rendered HTML contains
// the expected agent name, date, and DOCTYPE.

use std::path::PathBuf;
use std::process::Command;

use chrono::{NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
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

/// Build a UTC RFC 3339 timestamp for "today" (UTC) at the given hour,
/// so the row lands inside `today_range(--tz UTC)`.
fn today_ts(hour: u32, min: u32) -> String {
    let now = Utc::now().date_naive();
    let dt = NaiveDateTime::new(now, NaiveTime::from_hms_opt(hour, min, 0).unwrap());
    Utc.from_utc_datetime(&dt)
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

#[test]
fn cli_today_html_to_stdout_contains_doctype_and_agent() {
    let (_dir, db) = fixture_db(&[
        (
            today_ts(10, 5).as_str(),
            "claude-code",
            "Edit",
            "s1",
            "src/foo.rs",
            "/proj/a",
        ),
        (
            today_ts(10, 12).as_str(),
            "claude-code",
            "Read",
            "s1",
            "src/bar.rs",
            "/proj/a",
        ),
    ]);

    // Binary writes the full document on stdout when `--out -`.
    let output = Command::new(env!("CARGO_BIN_EXE_fluxmirror"))
        .args([
            "today", "--tz", "UTC", "--lang", "english", "--html", "--out", "-",
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
    assert!(
        stdout.starts_with("<!DOCTYPE html>"),
        "missing DOCTYPE; first 80 bytes: {}",
        &stdout[..80.min(stdout.len())]
    );
    assert!(stdout.contains("</html>"));
    assert!(stdout.contains("claude-code"), "missing agent name");
    let date = Utc::now().date_naive().format("%Y-%m-%d").to_string();
    assert!(
        stdout.contains(&date),
        "missing today's date label: {}",
        date
    );
}

#[test]
fn cli_today_format_html_and_html_flag_match() {
    // The `--html` shorthand must produce the same bytes as
    // `--format html`. Otherwise the documented equivalence breaks.
    let (_dir, db) = fixture_db(&[(
        today_ts(11, 15).as_str(),
        "gemini-cli",
        "read_file",
        "s1",
        "src/bar.rs",
        "/proj/a",
    )]);

    let a = Command::new(env!("CARGO_BIN_EXE_fluxmirror"))
        .args(["today", "--tz", "UTC", "--lang", "english", "--html", "--out", "-"])
        .arg("--db")
        .arg(&db)
        .output()
        .unwrap();
    let b = Command::new(env!("CARGO_BIN_EXE_fluxmirror"))
        .args([
            "today", "--tz", "UTC", "--lang", "english", "--format", "html", "--out", "-",
        ])
        .arg("--db")
        .arg(&db)
        .output()
        .unwrap();

    assert_eq!(a.stdout, b.stdout, "--html and --format html diverged");
}

#[test]
fn cli_today_explicit_non_default_format_overrides_html_flag() {
    // Spec: when both `--format` and `--html` are given, `--format`
    // wins. Use `--format json` (which is reserved → exit 2) plus
    // `--html` and confirm the binary still hits the not-implemented
    // path rather than rendering HTML.
    let (_dir, db) = fixture_db(&[(
        today_ts(12, 0).as_str(),
        "claude-code",
        "Edit",
        "s1",
        "src/foo.rs",
        "/proj/a",
    )]);

    let output = Command::new(env!("CARGO_BIN_EXE_fluxmirror"))
        .args([
            "today", "--tz", "UTC", "--lang", "english", "--format", "json", "--html",
        ])
        .arg("--db")
        .arg(&db)
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(2),
        "expected exit 2 (json reserved): stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("not yet implemented"),
        "expected not-implemented stderr, got: {}",
        stderr
    );
}

#[test]
fn cli_today_korean_lang_appears_in_card() {
    let (_dir, db) = fixture_db(&[(
        today_ts(9, 0).as_str(),
        "claude-code",
        "Bash",
        "s1",
        "echo hi",
        "/proj",
    )]);

    let output = Command::new(env!("CARGO_BIN_EXE_fluxmirror"))
        .args([
            "today", "--tz", "UTC", "--lang", "korean", "--html", "--out", "-",
        ])
        .arg("--db")
        .arg(&db)
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("\u{c624}\u{b298}\u{c758} \u{c791}\u{c5c5}"),
        "missing Korean today title in card");
    let _ = NaiveDate::from_ymd_opt(2026, 1, 1); // silence unused import
}
