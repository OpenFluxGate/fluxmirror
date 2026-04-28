//! Integration tests for Phase 3 M7 — output-surface redaction.
//!
//! Approach: seed a fixture `events.db` with rows whose `detail` and
//! `cwd` columns hold known secrets, run every report subcommand
//! against it (via the compiled binary), and assert the rendered output
//! never leaks the original tokens.
//!
//! At the end we re-open the same DB and confirm the raw bytes are
//! still intact — the capture-side store is the source of truth and
//! must never be touched by the presentation-layer scrubber.

use std::path::PathBuf;
use std::process::Command;

use chrono::{Duration, Utc};
use fluxmirror_store::SqliteStore;
use rusqlite::{params, Connection};
use tempfile::TempDir;

const SECRET_AWS_KEY: &str = "AKIAIOSFODNN7EXAMPLE";
const SECRET_GHP: &str = "ghp_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const SECRET_ENV_PATH: &str = "/repo/.env.production";

fn fluxmirror_bin() -> &'static str {
    env!("CARGO_BIN_EXE_fluxmirror")
}

fn seed_db(path: &PathBuf) {
    let _store = SqliteStore::open(path).expect("init schema");
    let conn = Connection::open(path).expect("open db");
    let now = Utc::now();
    let yesterday = now - Duration::days(1);
    let two_days = now - Duration::days(2);

    let stamps = [
        // (ts, agent, tool, detail, cwd, session)
        (
            now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            "claude-code",
            "Bash",
            format!("export AWS_ACCESS_KEY_ID={}", SECRET_AWS_KEY),
            SECRET_ENV_PATH.to_string(),
            "s-today".to_string(),
        ),
        (
            yesterday.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            "claude-code",
            "Bash",
            format!("curl -H 'token={}' https://api.github.com", SECRET_GHP),
            SECRET_ENV_PATH.to_string(),
            "s-yest".to_string(),
        ),
        (
            two_days.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            "gemini-cli",
            "shell",
            format!("password=hunter2-secret-pw export AWS_ACCESS_KEY_ID={}", SECRET_AWS_KEY),
            "/repo".to_string(),
            "g-1".to_string(),
        ),
    ];

    for (ts, agent, tool, detail, cwd, session) in stamps.iter() {
        conn.execute(
            "INSERT INTO agent_events \
             (ts, agent, session, tool, tool_canonical, tool_class, detail, \
              cwd, host, user, schema_version, raw_json) \
             VALUES (?1, ?2, ?3, ?4, ?4, 'Shell', ?5, ?6, 'h', 'u', 1, '{}')",
            params![ts, agent, session, tool, detail, cwd],
        )
        .expect("insert row");
    }
}

/// Run the `fluxmirror` binary with `args`, plus a clean env so the
/// merged config never picks up the host's own ~/.fluxmirror state. We
/// point HOME at the per-test tempdir so there's no user-file layer.
fn run_subcmd(home: &PathBuf, db: &PathBuf, args: &[&str]) -> (String, String, i32) {
    let out = Command::new(fluxmirror_bin())
        .args(args)
        .arg("--db")
        .arg(db)
        .env("HOME", home)
        .env_remove("USERPROFILE")
        .env_remove("LANG")
        .env_remove("FLUXMIRROR_LANGUAGE")
        .env_remove("FLUXMIRROR_TIMEZONE")
        .env_remove("FLUXMIRROR_DB")
        .current_dir(home) // so .fluxmirror.toml lookup hits an empty dir
        .output()
        .expect("spawn fluxmirror");
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        out.status.code().unwrap_or(-1),
    )
}

fn assert_no_leaks(stdout: &str, label: &str) {
    assert!(
        !stdout.contains(SECRET_AWS_KEY),
        "{label}: leaked AWS key in stdout:\n{stdout}"
    );
    assert!(
        !stdout.contains(SECRET_GHP),
        "{label}: leaked GitHub PAT in stdout:\n{stdout}"
    );
    assert!(
        !stdout.contains("hunter2-secret-pw"),
        "{label}: leaked kv_secret value in stdout:\n{stdout}"
    );
    assert!(
        !stdout.contains(".env.production"),
        "{label}: leaked .env path in stdout:\n{stdout}"
    );
    // And SOMETHING must have been redacted, otherwise the report didn't
    // surface the seeded rows at all.
    assert!(
        stdout.contains("[REDACTED:"),
        "{label}: no [REDACTED:...] sentinel in stdout (did the report run?):\n{stdout}"
    );
}

#[test]
fn today_scrubs_every_secret_class() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().to_path_buf();
    let db = tmp.path().join("events.db");
    seed_db(&db);

    let (stdout, _stderr, code) =
        run_subcmd(&home, &db, &["today", "--tz", "UTC", "--lang", "english"]);
    assert_eq!(code, 0, "exit non-zero:\n{stdout}");
    assert_no_leaks(&stdout, "today");
}

#[test]
fn yesterday_scrubs_every_secret_class() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().to_path_buf();
    let db = tmp.path().join("events.db");
    seed_db(&db);

    let (stdout, _stderr, code) = run_subcmd(
        &home,
        &db,
        &["yesterday", "--tz", "UTC", "--lang", "english"],
    );
    assert_eq!(code, 0, "exit non-zero:\n{stdout}");
    assert_no_leaks(&stdout, "yesterday");
}

#[test]
fn week_scrubs_every_secret_class() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().to_path_buf();
    let db = tmp.path().join("events.db");
    seed_db(&db);

    let (stdout, _stderr, code) = run_subcmd(
        &home,
        &db,
        &["week", "--tz", "UTC", "--lang", "english", "--no-git-narrative"],
    );
    assert_eq!(code, 0, "exit non-zero:\n{stdout}");
    assert_no_leaks(&stdout, "week");
}

#[test]
fn agents_scrubs_every_secret_class() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().to_path_buf();
    let db = tmp.path().join("events.db");
    seed_db(&db);

    let (stdout, _stderr, code) =
        run_subcmd(&home, &db, &["agents", "--tz", "UTC", "--lang", "english"]);
    assert_eq!(code, 0, "exit non-zero:\n{stdout}");
    // agents only surfaces aggregate counts (no detail field), so this
    // report's secret leakage surface is small. Just ensure no token
    // leak — it's fine if no [REDACTED:] sentinel appears.
    assert!(!stdout.contains(SECRET_AWS_KEY), "leaked AWS key:\n{stdout}");
    assert!(!stdout.contains(SECRET_GHP), "leaked PAT:\n{stdout}");
}

#[test]
fn agent_subcommand_scrubs_secrets() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().to_path_buf();
    let db = tmp.path().join("events.db");
    seed_db(&db);

    let (stdout, _stderr, code) = run_subcmd(
        &home,
        &db,
        &[
            "agent",
            "claude-code",
            "--period",
            "week",
            "--tz",
            "UTC",
            "--lang",
            "english",
        ],
    );
    assert_eq!(code, 0, "exit non-zero:\n{stdout}");
    assert_no_leaks(&stdout, "agent");
}

#[test]
fn compare_scrubs_every_secret_class() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().to_path_buf();
    let db = tmp.path().join("events.db");
    seed_db(&db);

    let (stdout, _stderr, code) = run_subcmd(
        &home,
        &db,
        &["compare", "--tz", "UTC", "--lang", "english"],
    );
    assert_eq!(code, 0, "exit non-zero:\n{stdout}");
    // compare only surfaces aggregate metric counts (no detail field).
    // Verify that no token sneaks through.
    assert!(!stdout.contains(SECRET_AWS_KEY));
    assert!(!stdout.contains(SECRET_GHP));
}

#[test]
fn week_html_card_scrubs_every_secret_class() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().to_path_buf();
    let db = tmp.path().join("events.db");
    seed_db(&db);

    let (stdout, _stderr, code) = run_subcmd(
        &home,
        &db,
        &[
            "week",
            "--tz",
            "UTC",
            "--lang",
            "english",
            "--no-git-narrative",
            "--format",
            "html",
            "--out",
            "-",
        ],
    );
    assert_eq!(code, 0, "exit non-zero:\n{stdout}");
    assert!(stdout.contains("<!DOCTYPE html>"));
    assert_no_leaks(&stdout, "week html");
}

#[test]
fn today_html_card_scrubs_every_secret_class() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().to_path_buf();
    let db = tmp.path().join("events.db");
    seed_db(&db);

    let (stdout, _stderr, code) = run_subcmd(
        &home,
        &db,
        &[
            "today",
            "--tz",
            "UTC",
            "--lang",
            "english",
            "--format",
            "html",
            "--out",
            "-",
        ],
    );
    assert_eq!(code, 0, "exit non-zero:\n{stdout}");
    assert!(stdout.contains("<!DOCTYPE html>"));
    assert_no_leaks(&stdout, "today html");
}

#[test]
fn db_integrity_secrets_survive_after_scrubbed_reports() {
    // Run a full report cycle, then re-open events.db and confirm the
    // raw secrets are still in the detail column. The scrubber must
    // never reach back into the source-of-truth store.
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().to_path_buf();
    let db = tmp.path().join("events.db");
    seed_db(&db);

    // Run several reports back-to-back; each one passes through scrub.
    for argv in [
        vec!["today", "--tz", "UTC", "--lang", "english"],
        vec!["yesterday", "--tz", "UTC", "--lang", "english"],
        vec![
            "week",
            "--tz",
            "UTC",
            "--lang",
            "english",
            "--no-git-narrative",
        ],
    ] {
        let (_out, _err, code) = run_subcmd(&home, &db, &argv);
        assert_eq!(code, 0, "subcommand failed: {:?}", argv);
    }

    let conn = Connection::open(&db).expect("re-open db");
    let mut stmt = conn
        .prepare("SELECT detail, cwd FROM agent_events ORDER BY ts")
        .unwrap();
    let rows: Vec<(String, String)> = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(rows.len(), 3);

    // Every seeded raw secret must still be present somewhere in the
    // unmodified rows.
    let bag: String = rows
        .iter()
        .flat_map(|(d, c)| [d.as_str(), c.as_str()])
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        bag.contains(SECRET_AWS_KEY),
        "events.db lost the seeded AWS key — capture path was scrubbed"
    );
    assert!(
        bag.contains(SECRET_GHP),
        "events.db lost the seeded GitHub PAT"
    );
    assert!(
        bag.contains("hunter2-secret-pw"),
        "events.db lost the seeded password value"
    );
    assert!(
        bag.contains(".env.production"),
        "events.db lost the seeded .env path"
    );
}

#[test]
fn user_pattern_layered_via_project_toml() {
    // Drop a `.fluxmirror.toml` next to the cwd and confirm a custom
    // user pattern shows up in the rendered output as `[REDACTED:user]`.
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().to_path_buf();
    let db = tmp.path().join("events.db");
    let _store = SqliteStore::open(&db).unwrap();

    // One row whose detail leaks an internal-only token shape that
    // none of the built-ins flag.
    let conn = Connection::open(&db).unwrap();
    let now = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    conn.execute(
        "INSERT INTO agent_events \
         (ts, agent, session, tool, tool_canonical, tool_class, detail, \
          cwd, host, user, schema_version, raw_json) \
         VALUES (?1, 'claude-code', 's', 'Bash', 'Bash', 'Shell', \
                 'echo INTERNAL-TOKEN-12345', '/repo', 'h', 'u', 1, '{}')",
        params![now],
    )
    .unwrap();

    // .fluxmirror.toml in cwd carries the user pattern. Config::load()
    // walks the project layer when current_dir is set to `home`.
    std::fs::write(
        home.join(".fluxmirror.toml"),
        "[redaction]\npatterns = [\"INTERNAL-TOKEN-\\\\d+\"]\n",
    )
    .unwrap();

    let (stdout, _stderr, code) =
        run_subcmd(&home, &db, &["today", "--tz", "UTC", "--lang", "english"]);
    assert_eq!(code, 0, "exit non-zero:\n{stdout}");
    assert!(
        stdout.contains("[REDACTED:user]"),
        "missing user-pattern mask in:\n{stdout}"
    );
    assert!(
        !stdout.contains("INTERNAL-TOKEN-12345"),
        "user pattern failed to scrub:\n{stdout}"
    );
}
