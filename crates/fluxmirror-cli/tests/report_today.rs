// Integration test for `fluxmirror today`.
//
// Seeds a tempdir SQLite DB with fixture rows spanning multiple agents,
// tools, and timestamps inside today's local window, then invokes the
// binary via `Command::new(env!("CARGO_BIN_EXE_fluxmirror"))` and
// asserts the rendered report contains the expected sections, agents,
// file paths, and language-specific titles.

use std::path::PathBuf;
use std::process::Command;

use chrono::{Duration, Utc};
use chrono_tz::Tz;
use fluxmirror_store::SqliteStore;
use rusqlite::{params, Connection};
use tempfile::TempDir;

/// Build a fixture DB anchored to "today" in UTC. The binary uses the
/// today_range helper, which evaluates `Utc::now()` at runtime; we
/// place the rows at well-defined offsets within today so they always
/// fall in the queried window even on slow machines.
///
/// The fixture covers three agents, mixed tools (Edit/Read/Bash plus
/// gemini snake_case `read_file`), two distinct working directories
/// (one with ≥5 calls, one with 1) and timestamps that hit two
/// distinct local hours so the histogram is visible.
fn fixture_db_busy() -> (TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.db");
    let _store = SqliteStore::open(&path).unwrap();
    let conn = Connection::open(&path).unwrap();

    // Use UTC midday ± hours so we land inside today's UTC window when
    // the binary computes today_range with --tz=UTC.
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

    // Define 25 rows: each tuple is (offset_minutes, agent, tool, session, detail, cwd).
    let rows: Vec<(i64, &str, &str, &str, &str, &str)> = vec![
        // claude-code — 12 events, two hours apart
        (-90, "claude-code", "Edit", "c-s1", "src/foo.rs", "/proj/a"),
        (-85, "claude-code", "Edit", "c-s1", "src/foo.rs", "/proj/a"),
        (-80, "claude-code", "Edit", "c-s1", "src/bar.rs", "/proj/a"),
        (-75, "claude-code", "Read", "c-s1", "src/baz.rs", "/proj/a"),
        (-70, "claude-code", "Read", "c-s1", "src/qux.rs", "/proj/a"),
        (-65, "claude-code", "Bash", "c-s1", "cargo build", "/proj/a"),
        (-60, "claude-code", "Bash", "c-s1", "cargo test", "/proj/a"),
        (-55, "claude-code", "Edit", "c-s2", "src/foo.rs", "/proj/a"),
        (-50, "claude-code", "Edit", "c-s2", "src/foo.rs", "/proj/a"),
        (-45, "claude-code", "Bash", "c-s2", "git status", "/proj/a"),
        (-30, "claude-code", "Read", "c-s2", "Cargo.toml", "/proj/a"),
        (-25, "claude-code", "Edit", "c-s2", "Cargo.toml", "/proj/a"),
        // gemini-cli — 5 events with snake_case tools
        (10, "gemini-cli", "read_file", "g1", "src/foo.rs", "/proj/b"),
        (15, "gemini-cli", "read_file", "g1", "README.md", "/proj/b"),
        (20, "gemini-cli", "edit_file", "g1", "README.md", "/proj/b"),
        (25, "gemini-cli", "edit_file", "g1", "README.md", "/proj/b"),
        (30, "gemini-cli", "run_shell_command", "g1", "ls -al", "/proj/b"),
        // qwen-code — 8 events, mostly writes
        (35, "qwen-code", "Write", "q-s1", "docs/note.md", "/proj/a"),
        (40, "qwen-code", "Edit", "q-s1", "docs/note.md", "/proj/a"),
        (45, "qwen-code", "Edit", "q-s1", "docs/note.md", "/proj/a"),
        (50, "qwen-code", "Edit", "q-s1", "docs/note.md", "/proj/a"),
        (55, "qwen-code", "Read", "q-s1", "docs/note.md", "/proj/a"),
        (60, "qwen-code", "Bash", "q-s1", "ls", "/proj/a"),
        (65, "qwen-code", "Bash", "q-s1", "pwd", "/proj/a"),
        (70, "qwen-code", "Edit", "q-s1", "docs/spec.md", "/proj/a"),
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

/// Empty-DB fixture used to exercise the "no activity" branch.
/// The threshold was lowered from 5 to 1 so any non-empty window renders
/// the full report; only a truly empty window emits the dismissal line.
fn fixture_db_sparse() -> (TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.db");
    let _store = SqliteStore::open(&path).unwrap();
    // Intentionally empty — no rows inserted.
    (dir, path)
}

fn fluxmirror_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_fluxmirror"))
}

#[test]
fn today_human_english_lists_agents_files_and_date() {
    let (_dir, db) = fixture_db_busy();

    let output = fluxmirror_bin()
        .arg("today")
        .arg("--db")
        .arg(&db)
        .arg("--tz")
        .arg("UTC")
        .arg("--lang")
        .arg("english")
        .arg("--format")
        .arg("human")
        .output()
        .expect("spawn fluxmirror today");

    assert!(
        output.status.success(),
        "non-zero exit: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();

    // Date appears in YYYY-MM-DD form. Compute today's UTC date the
    // same way the binary does (since we use --tz=UTC).
    let tz: Tz = "UTC".parse().unwrap();
    let today = Utc::now().with_timezone(&tz).date_naive();
    let date_str = today.format("%Y-%m-%d").to_string();
    assert!(
        stdout.contains(&date_str),
        "missing today's date {date_str}:\n{stdout}"
    );

    // Title.
    assert!(stdout.contains("Today's Work"), "missing en title:\n{stdout}");

    // Every agent name from the fixture must appear.
    for agent in ["claude-code", "gemini-cli", "qwen-code"] {
        assert!(
            stdout.contains(agent),
            "missing agent {agent} in:\n{stdout}"
        );
    }

    // A known file path from the fixture must appear in the
    // edited-files table.
    assert!(
        stdout.contains("src/foo.rs"),
        "missing fixture file:\n{stdout}"
    );

    // Activity / hour-distribution headings present.
    assert!(stdout.contains("Activity"), "missing activity heading:\n{stdout}");
    assert!(stdout.contains("Hour distribution"), "missing hours:\n{stdout}");

    // Three insight bullets at most. Rule 1 (busiest hour) must fire.
    assert!(
        stdout.contains("Most productive hour"),
        "missing busiest-hour insight:\n{stdout}"
    );
}

#[test]
fn today_korean_title_differs_from_english() {
    let (_dir, db) = fixture_db_busy();

    let en = fluxmirror_bin()
        .args(["today", "--tz", "UTC", "--lang", "english"])
        .arg("--db")
        .arg(&db)
        .output()
        .unwrap();
    let ko = fluxmirror_bin()
        .args(["today", "--tz", "UTC", "--lang", "korean"])
        .arg("--db")
        .arg(&db)
        .output()
        .unwrap();

    assert!(en.status.success());
    assert!(ko.status.success());
    let en_out = String::from_utf8(en.stdout).unwrap();
    let ko_out = String::from_utf8(ko.stdout).unwrap();

    assert!(en_out.contains("Today's Work"));
    assert!(ko_out.contains("오늘의 작업"), "ko output:\n{ko_out}");
    assert_ne!(en_out, ko_out, "en and ko should differ");
}

#[test]
fn today_sparse_fixture_emits_limited_activity_line() {
    let (_dir, db) = fixture_db_sparse();

    let output = fluxmirror_bin()
        .args(["today", "--tz", "UTC", "--lang", "english"])
        .arg("--db")
        .arg(&db)
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("Limited activity today."),
        "expected limited-activity line in:\n{stdout}"
    );
}

#[test]
fn today_format_json_is_reserved_but_unimplemented() {
    let (_dir, db) = fixture_db_busy();

    let output = fluxmirror_bin()
        .args(["today", "--tz", "UTC", "--lang", "english", "--format", "json"])
        .arg("--db")
        .arg(&db)
        .output()
        .unwrap();

    assert!(!output.status.success(), "json format should exit non-zero");
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("not yet implemented"),
        "expected stub message in stderr:\n{stderr}"
    );
}
