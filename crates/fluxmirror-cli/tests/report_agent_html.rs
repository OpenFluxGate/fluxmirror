// Integration test for `fluxmirror agent --format html` (M5.4).
//
// Fixture has rows for two agents in the today window. The agent
// subcommand filtered to one agent must:
//   - emit a complete HTML document
//   - include the requested agent's name in the title
//   - NOT mention the other agent anywhere in the document body
//     (the per-agent table is dropped, but we want extra confidence
//     the filter also drops cross-agent file edits / sessions).

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

#[test]
fn cli_agent_html_filters_to_requested_agent() {
    let (_dir, db) = fixture_db(&[
        (
            today_ts(10, 0).as_str(),
            "claude-code",
            "Edit",
            "s1",
            "src/claude_only.rs",
            "/proj/a",
        ),
        (
            today_ts(10, 5).as_str(),
            "claude-code",
            "Bash",
            "s1",
            "echo claude-only",
            "/proj/a",
        ),
        (
            today_ts(11, 0).as_str(),
            "gemini-cli",
            "edit_file",
            "g1",
            "src/gemini_secret.rs",
            "/proj/b",
        ),
        (
            today_ts(11, 5).as_str(),
            "gemini-cli",
            "run_shell_command",
            "g1",
            "echo gemini-secret",
            "/proj/b",
        ),
    ]);

    let output = Command::new(env!("CARGO_BIN_EXE_fluxmirror"))
        .args([
            "agent", "claude-code",
            "--tz", "UTC",
            "--lang", "english",
            "--period", "today",
            "--html",
            "--out", "-",
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
    assert!(stdout.contains("claude-code"), "missing requested agent");
    // Filter sanity: the other agent's identifying detail strings must
    // not leak into the filtered card.
    assert!(
        !stdout.contains("gemini_secret.rs"),
        "other agent's file leaked"
    );
    assert!(
        !stdout.contains("gemini-secret"),
        "other agent's shell text leaked"
    );
    assert!(
        !stdout.contains("gemini-cli"),
        "other agent's name leaked"
    );
}

#[test]
fn cli_agent_week_html_renders_per_day_breakdown() {
    let (_dir, db) = fixture_db(&[
        (
            today_ts(9, 0).as_str(),
            "claude-code",
            "Edit",
            "s1",
            "src/foo.rs",
            "/proj/a",
        ),
        (
            today_ts(10, 0).as_str(),
            "claude-code",
            "Read",
            "s1",
            "src/bar.rs",
            "/proj/a",
        ),
    ]);

    let output = Command::new(env!("CARGO_BIN_EXE_fluxmirror"))
        .args([
            "agent", "claude-code",
            "--tz", "UTC",
            "--lang", "english",
            "--period", "week",
            "--html",
            "--out", "-",
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
    assert!(stdout.contains("claude-code"));
    assert!(stdout.contains("Last 7 Days"));
}
