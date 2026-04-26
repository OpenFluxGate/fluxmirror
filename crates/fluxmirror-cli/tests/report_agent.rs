// Integration test for `fluxmirror agent <name>`.
//
// Verifies the single-agent filter:
// - the requested agent's data is present
// - other agents' data is absent (no cross-agent leakage)
// - title carries the agent prefix
// - the no-data branch fires for an unknown agent / empty window

use std::path::PathBuf;
use std::process::Command;

use chrono::{Duration, Utc};
use fluxmirror_store::SqliteStore;
use rusqlite::{params, Connection};
use tempfile::TempDir;

fn fixture_db_mixed_agents() -> (TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.db");
    let _store = SqliteStore::open(&path).unwrap();
    let conn = Connection::open(&path).unwrap();

    let now = Utc::now();
    let today_noon = now
        .date_naive()
        .and_hms_opt(12, 0, 0)
        .unwrap()
        .and_utc();
    let stamp = |minutes: i64| {
        (today_noon + Duration::minutes(minutes))
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
    };

    let rows: Vec<(i64, &str, &str, &str, &str, &str)> = vec![
        // claude-code: 6 calls, distinctive file path
        (-60, "claude-code", "Edit", "c1", "src/CLAUDE_ONLY.rs", "/proj/c"),
        (-55, "claude-code", "Edit", "c1", "src/CLAUDE_ONLY.rs", "/proj/c"),
        (-50, "claude-code", "Edit", "c1", "src/CLAUDE_ONLY.rs", "/proj/c"),
        (-45, "claude-code", "Read", "c1", "Cargo.toml", "/proj/c"),
        (-40, "claude-code", "Bash", "c1", "cargo test", "/proj/c"),
        (-35, "claude-code", "Bash", "c1", "cargo build", "/proj/c"),
        // gemini-cli: 4 calls, distinctive file path
        (10, "gemini-cli", "edit_file", "g1", "src/GEMINI_ONLY.md", "/proj/g"),
        (15, "gemini-cli", "edit_file", "g1", "src/GEMINI_ONLY.md", "/proj/g"),
        (20, "gemini-cli", "read_file", "g1", "src/GEMINI_ONLY.md", "/proj/g"),
        (25, "gemini-cli", "run_shell_command", "g1", "ls -al", "/proj/g"),
        // qwen-code: 3 calls, distinctive file path
        (35, "qwen-code", "Edit", "q1", "src/QWEN_ONLY.rs", "/proj/q"),
        (40, "qwen-code", "Edit", "q1", "src/QWEN_ONLY.rs", "/proj/q"),
        (45, "qwen-code", "Read", "q1", "src/QWEN_ONLY.rs", "/proj/q"),
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

fn fluxmirror_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_fluxmirror"))
}

#[test]
fn agent_today_filters_to_named_agent_only() {
    let (_dir, db) = fixture_db_mixed_agents();

    let output = fluxmirror_bin()
        .args(["agent", "claude-code", "--period", "today", "--tz", "UTC", "--lang", "english"])
        .arg("--db")
        .arg(&db)
        .output()
        .expect("spawn fluxmirror agent");
    assert!(
        output.status.success(),
        "non-zero exit: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();

    // The requested agent's distinctive file must appear.
    assert!(
        stdout.contains("src/CLAUDE_ONLY.rs"),
        "missing claude-only file:\n{stdout}"
    );
    // Other agents' distinctive files must NOT appear.
    assert!(
        !stdout.contains("src/GEMINI_ONLY.md"),
        "gemini file leaked into claude-code report:\n{stdout}"
    );
    assert!(
        !stdout.contains("src/QWEN_ONLY.rs"),
        "qwen file leaked into claude-code report:\n{stdout}"
    );
    // Other agents' names must NOT appear.
    assert!(
        !stdout.contains("gemini-cli"),
        "gemini-cli leaked into claude-code report:\n{stdout}"
    );
    assert!(
        !stdout.contains("qwen-code"),
        "qwen-code leaked into claude-code report:\n{stdout}"
    );

    // Title must carry the agent prefix.
    assert!(
        stdout.starts_with("# claude-code:"),
        "missing agent prefix in title:\n{stdout}"
    );
    // Body should reference today's heading (Today's Work).
    assert!(stdout.contains("Today's Work"));
}

#[test]
fn agent_week_filters_and_keeps_per_day_section() {
    let (_dir, db) = fixture_db_mixed_agents();

    let output = fluxmirror_bin()
        .args(["agent", "gemini-cli", "--period", "week", "--tz", "UTC", "--lang", "english"])
        .arg("--db")
        .arg(&db)
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();

    assert!(stdout.contains("gemini-cli:"), "missing prefix:\n{stdout}");
    assert!(stdout.contains("Last 7 Days"), "missing week title:\n{stdout}");
    assert!(stdout.contains("Per-day totals"), "missing per-day:\n{stdout}");
    assert!(
        stdout.contains("src/GEMINI_ONLY.md"),
        "missing gemini file:\n{stdout}"
    );
    // Other agents' distinctive paths must not appear.
    assert!(!stdout.contains("src/CLAUDE_ONLY.rs"));
    assert!(!stdout.contains("src/QWEN_ONLY.rs"));
}

#[test]
fn agent_unknown_name_emits_no_activity_line() {
    let (_dir, db) = fixture_db_mixed_agents();
    let output = fluxmirror_bin()
        .args(["agent", "no-such-agent", "--period", "today", "--tz", "UTC", "--lang", "english"])
        .arg("--db")
        .arg(&db)
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.starts_with("no-such-agent: "),
        "missing per-agent prefix:\n{stdout}"
    );
}

#[test]
fn agent_korean_translates_titles() {
    let (_dir, db) = fixture_db_mixed_agents();
    let ko = fluxmirror_bin()
        .args(["agent", "claude-code", "--tz", "UTC", "--lang", "korean"])
        .arg("--db")
        .arg(&db)
        .output()
        .unwrap();
    assert!(ko.status.success());
    let s = String::from_utf8(ko.stdout).unwrap();
    assert!(s.contains("claude-code:"));
    assert!(s.contains("오늘의 작업"), "missing ko title:\n{s}");
}

#[test]
fn agent_format_json_is_reserved_but_unimplemented() {
    let (_dir, db) = fixture_db_mixed_agents();
    let output = fluxmirror_bin()
        .args([
            "agent",
            "claude-code",
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
}
