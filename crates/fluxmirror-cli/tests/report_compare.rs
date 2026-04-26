// Integration test for `fluxmirror compare`.
//
// Seeds rows in both today's and yesterday's UTC windows, then asserts:
// - exit 0
// - title "Today vs Yesterday"
// - the Δ column header is present
// - at least one ↑ or ↓ arrow when fixture data has a clear difference
// - empty DB → "Not enough activity to compare" line + exit 0

use std::path::PathBuf;
use std::process::Command;

use chrono::{Duration, Utc};
use fluxmirror_store::SqliteStore;
use rusqlite::{params, Connection};
use tempfile::TempDir;

fn fixture_db_today_heavier_than_yesterday() -> (TempDir, PathBuf) {
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
    let yest_noon = today_noon - Duration::days(1);

    let stamp = |base: chrono::DateTime<Utc>, minutes: i64| {
        (base + Duration::minutes(minutes))
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
    };

    // 12 today, 4 yesterday → today is +200% vs yesterday → ↑ arrow.
    let mut rows: Vec<(String, &str, &str, &str, &str, &str)> = Vec::new();
    for i in 0..12 {
        rows.push((
            stamp(today_noon, i * 5),
            "claude-code",
            "Edit",
            "ct1",
            "src/foo.rs",
            "/proj/a",
        ));
    }
    for i in 0..4 {
        rows.push((
            stamp(yest_noon, i * 5),
            "claude-code",
            "Read",
            "cy1",
            "src/foo.rs",
            "/proj/a",
        ));
    }

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
fn compare_human_lists_metric_table_and_arrow() {
    let (_dir, db) = fixture_db_today_heavier_than_yesterday();

    let output = fluxmirror_bin()
        .args(["compare", "--tz", "UTC", "--lang", "english"])
        .arg("--db")
        .arg(&db)
        .output()
        .expect("spawn fluxmirror compare");
    assert!(
        output.status.success(),
        "non-zero exit: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();

    assert!(stdout.contains("Today vs Yesterday"), "missing title:\n{stdout}");
    assert!(stdout.contains("| Δ |"), "missing Δ column:\n{stdout}");
    assert!(
        stdout.contains("↑") || stdout.contains("↓"),
        "expected an arrow indicator:\n{stdout}"
    );
    assert!(stdout.contains("Total calls"), "missing total row:\n{stdout}");
    assert!(stdout.contains("Edits"), "missing edits row:\n{stdout}");
    assert!(stdout.contains("Reads"), "missing reads row:\n{stdout}");
    assert!(
        stdout.contains("Calls: today is up"),
        "missing trend insight:\n{stdout}"
    );
}

#[test]
fn compare_korean_translates_titles_and_words() {
    let (_dir, db) = fixture_db_today_heavier_than_yesterday();

    let output = fluxmirror_bin()
        .args(["compare", "--tz", "UTC", "--lang", "korean"])
        .arg("--db")
        .arg(&db)
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("오늘 vs 어제"), "missing ko title:\n{stdout}");
    assert!(stdout.contains("증가") || stdout.contains("감소"), "missing ko direction word:\n{stdout}");
}

#[test]
fn compare_empty_db_emits_no_data_line() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.db");
    let _store = SqliteStore::open(&path).unwrap();
    let output = fluxmirror_bin()
        .args(["compare", "--tz", "UTC", "--lang", "english"])
        .arg("--db")
        .arg(&path)
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("Not enough activity to compare"),
        "expected no-data line in:\n{stdout}"
    );
}

#[test]
fn compare_format_json_is_reserved_but_unimplemented() {
    let (_dir, db) = fixture_db_today_heavier_than_yesterday();
    let output = fluxmirror_bin()
        .args(["compare", "--tz", "UTC", "--lang", "english", "--format", "json"])
        .arg("--db")
        .arg(&db)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(2));
}
