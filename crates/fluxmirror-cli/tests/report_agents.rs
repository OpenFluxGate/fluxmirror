// Integration test for `fluxmirror agents`.
//
// The test seeds a tempdir SQLite DB with fixture rows spanning two
// agents over the last 7 days (relative to "now"), runs the binary,
// and asserts:
//   - exit code 0
//   - both agent names appear in the rendered table
//   - the busiest insight names the agent with more calls
//   - --lang en and --lang ko produce different titles
//   - --format json exits 2 with a "not yet implemented" stderr line
//
// We invoke the binary via `Command::new(env!("CARGO_BIN_EXE_fluxmirror"))`
// instead of pulling in `assert_cmd`. CARGO_BIN_EXE_<name> is set by
// cargo for every `[[bin]]` in the package and points at the freshly
// built test binary, so no extra dev-dep is needed.

use std::path::PathBuf;
use std::process::Command;

use chrono::{Duration, Utc};
use fluxmirror_store::SqliteStore;
use rusqlite::{params, Connection};
use tempfile::TempDir;

/// Build a fresh fixture DB inside a tempdir. Returns the dir guard
/// (drop = cleanup) and the absolute DB path. We anchor the rows to
/// `now - N days` so the 7-day window the binary computes always
/// covers them.
fn fixture_db() -> (TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.db");
    let _store = SqliteStore::open(&path).unwrap();
    let conn = Connection::open(&path).unwrap();

    let now = Utc::now();
    // Anchor everything 1-3 days back so DST / midnight edge cases at
    // "exactly now" never bite the test on slow runners.
    let day_minus = |n: i64| {
        (now - Duration::days(n))
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
    };

    // claude-code: 6 events across 2 sessions, 2 active days.
    let claude_rows: [(&str, &str, &str); 6] = [
        ("Bash", "c-s1", &"x".repeat(0)), // placeholder, overwritten below
        ("Bash", "c-s1", "x"),
        ("Edit", "c-s1", "x"),
        ("Edit", "c-s2", "x"),
        ("Read", "c-s2", "x"),
        ("Bash", "c-s2", "x"),
    ];
    // gemini-cli: 1 event, 1 session — should trigger the one-shot rule.
    // qwen-code: 5 events, all writes — should trigger write-heavy rule.

    for (i, (tool, session, _)) in claude_rows.iter().enumerate() {
        let ts = if i < 3 {
            day_minus(2)
        } else {
            day_minus(1)
        };
        conn.execute(
            "INSERT INTO agent_events \
             (ts, agent, session, tool, tool_canonical, tool_class, detail, \
              cwd, host, user, schema_version, raw_json) \
             VALUES (?1, 'claude-code', ?2, ?3, ?3, 'Other', 'd', '/tmp', \
                     'h', 'u', 1, '{}')",
            params![ts, session, tool],
        )
        .unwrap();
    }

    // gemini one-shot
    conn.execute(
        "INSERT INTO agent_events \
         (ts, agent, session, tool, tool_canonical, tool_class, detail, \
          cwd, host, user, schema_version, raw_json) \
         VALUES (?1, 'gemini-cli', 'g-s1', 'read_file', 'Read', 'Read', \
                 'd', '/tmp', 'h', 'u', 1, '{}')",
        params![day_minus(1)],
    )
    .unwrap();

    // qwen write-heavy: 5 Edit rows
    for i in 0..5 {
        conn.execute(
            "INSERT INTO agent_events \
             (ts, agent, session, tool, tool_canonical, tool_class, detail, \
              cwd, host, user, schema_version, raw_json) \
             VALUES (?1, 'qwen-code', ?2, 'Edit', 'Edit', 'Write', \
                     'd', '/tmp', 'h', 'u', 1, '{}')",
            params![day_minus(3), format!("q-s{}", i % 2)],
        )
        .unwrap();
    }

    (dir, path)
}

fn fluxmirror_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_fluxmirror"))
}

#[test]
fn agents_human_english_lists_both_agents_and_insight() {
    let (_dir, db) = fixture_db();

    let output = fluxmirror_bin()
        .arg("agents")
        .arg("--db")
        .arg(&db)
        .arg("--tz")
        .arg("UTC")
        .arg("--lang")
        .arg("english")
        // Force human format so test stays stable regardless of future
        // default changes.
        .arg("--format")
        .arg("human")
        .output()
        .expect("spawn fluxmirror agents");

    assert!(
        output.status.success(),
        "non-zero exit: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();

    assert!(stdout.contains("Agent Roster"), "missing en title:\n{stdout}");
    assert!(stdout.contains("claude-code"), "missing claude-code:\n{stdout}");
    assert!(stdout.contains("gemini-cli"), "missing gemini-cli:\n{stdout}");
    assert!(stdout.contains("qwen-code"), "missing qwen-code:\n{stdout}");
    assert!(
        stdout.contains("claude-code is the busiest"),
        "missing busiest insight:\n{stdout}"
    );
    assert!(
        stdout.contains("gemini-cli ran a single session"),
        "missing one-shot insight:\n{stdout}"
    );
    assert!(
        stdout.contains("qwen-code is write-heavy"),
        "missing write-heavy insight:\n{stdout}"
    );
    // Range header
    assert!(stdout.contains("Range: "), "missing range header:\n{stdout}");
}

#[test]
fn agents_korean_title_differs_from_english() {
    let (_dir, db) = fixture_db();

    let en = fluxmirror_bin()
        .args(["agents", "--tz", "UTC", "--lang", "english"])
        .arg("--db")
        .arg(&db)
        .output()
        .unwrap();
    let ko = fluxmirror_bin()
        .args(["agents", "--tz", "UTC", "--lang", "korean"])
        .arg("--db")
        .arg(&db)
        .output()
        .unwrap();

    assert!(en.status.success());
    assert!(ko.status.success());
    let en_out = String::from_utf8(en.stdout).unwrap();
    let ko_out = String::from_utf8(ko.stdout).unwrap();

    assert!(en_out.contains("Agent Roster"));
    assert!(ko_out.contains("에이전트 명세"), "ko output:\n{ko_out}");
    assert!(en_out != ko_out, "en and ko should differ");
}

#[test]
fn agents_empty_db_prints_no_activity_line() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.db");
    // Create an empty schema-only DB.
    let _store = SqliteStore::open(&path).unwrap();

    let output = fluxmirror_bin()
        .args(["agents", "--tz", "UTC", "--lang", "english"])
        .arg("--db")
        .arg(&path)
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("No agent activity in the last 7 days."),
        "expected empty-window message in:\n{stdout}"
    );
}

#[test]
fn agents_format_json_is_reserved_but_unimplemented() {
    let (_dir, db) = fixture_db();

    let output = fluxmirror_bin()
        .args(["agents", "--tz", "UTC", "--lang", "english", "--format", "json"])
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
