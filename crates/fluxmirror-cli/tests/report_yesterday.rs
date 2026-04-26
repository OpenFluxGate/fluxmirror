// Integration test for `fluxmirror yesterday`.
//
// Mirrors `report_today.rs` but anchors fixture rows 24h earlier so they
// fall inside yesterday's UTC window. Asserts:
//   - exit 0
//   - title "Yesterday's Work"
//   - yesterday's date in YYYY-MM-DD form
//   - localized title under --lang ko
//   - sparse (< 5 events) fixture trips the "no activity yesterday" line

use std::path::PathBuf;
use std::process::Command;

use chrono::{Duration, Utc};
use chrono_tz::Tz;
use fluxmirror_store::SqliteStore;
use rusqlite::{params, Connection};
use tempfile::TempDir;

fn fixture_db_yesterday_busy() -> (TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.db");
    let _store = SqliteStore::open(&path).unwrap();
    let conn = Connection::open(&path).unwrap();

    // Anchor everything around yesterday's UTC midday so we stay inside
    // yesterday's window even on slow runners.
    let now = Utc::now();
    let yesterday_noon = (now - Duration::days(1))
        .date_naive()
        .and_hms_opt(12, 0, 0)
        .unwrap()
        .and_utc();

    let stamp = |minutes: i64| {
        (yesterday_noon + Duration::minutes(minutes))
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
    };

    let rows: Vec<(i64, &str, &str, &str, &str, &str)> = vec![
        (-60, "claude-code", "Edit", "c-s1", "src/foo.rs", "/proj/a"),
        (-50, "claude-code", "Edit", "c-s1", "src/foo.rs", "/proj/a"),
        (-40, "claude-code", "Read", "c-s1", "src/baz.rs", "/proj/a"),
        (-30, "claude-code", "Bash", "c-s1", "cargo build", "/proj/a"),
        (-20, "claude-code", "Bash", "c-s1", "cargo test", "/proj/a"),
        (10, "gemini-cli", "read_file", "g1", "README.md", "/proj/b"),
        (20, "gemini-cli", "edit_file", "g1", "README.md", "/proj/b"),
        (30, "qwen-code", "Edit", "q-s1", "docs/note.md", "/proj/a"),
    ];

    for (off, agent, tool, session, detail, cwd) in rows {
        conn.execute(
            "INSERT INTO agent_events \
             (ts, agent, session, tool, tool_canonical, tool_class, detail, \
              cwd, host, user, schema_version, raw_json) \
             VALUES (?1, ?2, ?3, ?4, ?4, 'Other', ?5, ?6, 'h', 'u', 1, '{}')",
            params![stamp(off), agent, session, tool, detail, cwd],
        )
        .unwrap();
    }
    (dir, path)
}

fn fixture_db_yesterday_sparse() -> (TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.db");
    let _store = SqliteStore::open(&path).unwrap();
    // Intentionally empty — the threshold was lowered from 5 to 1 so only
    // a truly zero-row window emits the "no activity" dismissal line.
    (dir, path)
}

fn fluxmirror_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_fluxmirror"))
}

#[test]
fn yesterday_human_english_lists_agents_and_yesterday_date() {
    let (_dir, db) = fixture_db_yesterday_busy();

    let output = fluxmirror_bin()
        .args(["yesterday", "--tz", "UTC", "--lang", "english"])
        .arg("--db")
        .arg(&db)
        .arg("--format")
        .arg("human")
        .output()
        .expect("spawn fluxmirror yesterday");

    assert!(
        output.status.success(),
        "non-zero exit: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();

    // Yesterday's UTC date in YYYY-MM-DD form must appear in the title.
    let tz: Tz = "UTC".parse().unwrap();
    let yesterday = (Utc::now().with_timezone(&tz) - Duration::days(1)).date_naive();
    let date_str = yesterday.format("%Y-%m-%d").to_string();
    assert!(
        stdout.contains(&date_str),
        "missing yesterday's date {date_str}:\n{stdout}"
    );

    assert!(
        stdout.contains("Yesterday's Work"),
        "missing en title:\n{stdout}"
    );
    for agent in ["claude-code", "gemini-cli", "qwen-code"] {
        assert!(
            stdout.contains(agent),
            "missing agent {agent} in:\n{stdout}"
        );
    }
    assert!(stdout.contains("src/foo.rs"));
    assert!(stdout.contains("Activity"));
    assert!(stdout.contains("Hour distribution"));
}

#[test]
fn yesterday_korean_title_differs_from_english() {
    let (_dir, db) = fixture_db_yesterday_busy();

    let en = fluxmirror_bin()
        .args(["yesterday", "--tz", "UTC", "--lang", "english"])
        .arg("--db")
        .arg(&db)
        .output()
        .unwrap();
    let ko = fluxmirror_bin()
        .args(["yesterday", "--tz", "UTC", "--lang", "korean"])
        .arg("--db")
        .arg(&db)
        .output()
        .unwrap();

    assert!(en.status.success());
    assert!(ko.status.success());
    let en_out = String::from_utf8(en.stdout).unwrap();
    let ko_out = String::from_utf8(ko.stdout).unwrap();

    assert!(en_out.contains("Yesterday's Work"));
    assert!(ko_out.contains("어제의 작업"), "ko output:\n{ko_out}");
    assert_ne!(en_out, ko_out);
}

#[test]
fn yesterday_sparse_fixture_emits_no_activity_line() {
    let (_dir, db) = fixture_db_yesterday_sparse();

    let output = fluxmirror_bin()
        .args(["yesterday", "--tz", "UTC", "--lang", "english"])
        .arg("--db")
        .arg(&db)
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("No activity yesterday."),
        "expected no-activity line in:\n{stdout}"
    );
}

#[test]
fn yesterday_format_json_is_reserved_but_unimplemented() {
    let (_dir, db) = fixture_db_yesterday_busy();

    let output = fluxmirror_bin()
        .args([
            "yesterday",
            "--tz",
            "UTC",
            "--lang",
            "english",
            "--format",
            "json",
        ])
        .arg("--db")
        .arg(&db)
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("not yet implemented"),
        "expected stub message in stderr:\n{stderr}"
    );
}
