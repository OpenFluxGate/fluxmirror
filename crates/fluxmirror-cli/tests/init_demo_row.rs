// Integration test for `fluxmirror init --demo-row`.
//
// Verifies the M3 first-run friction-removal contract:
//   - `--non-interactive` (with the demo-row default ON) drops exactly
//     one synthetic `agent='setup'` row into a fresh DB.
//   - A second init invocation against the same DB is idempotent —
//     still exactly one `agent='setup'` row.
//   - `--no-demo-row` opts out — zero `agent='setup'` rows in the DB.
//
// Each test isolates the DB via a unique `FLUXMIRROR_DB` value pointed
// at a tempdir file. HOME is also redirected to a fresh tempdir so the
// config-write side effect lands in a sandbox.

use std::path::PathBuf;
use std::process::Command;

use rusqlite::Connection;
use tempfile::TempDir;

fn fluxmirror_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_fluxmirror"))
}

/// Build a freshly-tempdir'd HOME + DB path. Returned tempdirs are kept
/// alive by the caller so they outlive the binary invocation.
fn fresh_sandbox(label: &str) -> (TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join(format!("{label}.db"));
    (dir, db)
}

fn count_setup_rows(db: &PathBuf) -> i64 {
    let conn = Connection::open(db).unwrap();
    conn.query_row(
        "SELECT COUNT(*) FROM agent_events WHERE agent = 'setup'",
        [],
        |r| r.get(0),
    )
    .unwrap()
}

#[test]
fn non_interactive_default_inserts_exactly_one_demo_row() {
    let (home, db) = fresh_sandbox("default");
    let output = fluxmirror_bin()
        .env("HOME", home.path())
        .env_remove("USERPROFILE")
        .env("FLUXMIRROR_DB", &db)
        .args([
            "init",
            "--non-interactive",
            "--language",
            "english",
            "--timezone",
            "UTC",
        ])
        .output()
        .expect("spawn fluxmirror init");

    assert!(
        output.status.success(),
        "init exit non-zero: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("/fluxmirror:today"),
        "missing demo-row confirmation in stdout:\n{stdout}"
    );

    assert_eq!(count_setup_rows(&db), 1, "expected exactly one demo row");
}

#[test]
fn second_init_is_idempotent() {
    let (home, db) = fresh_sandbox("idempotent");

    for _ in 0..2 {
        let output = fluxmirror_bin()
            .env("HOME", home.path())
            .env_remove("USERPROFILE")
            .env("FLUXMIRROR_DB", &db)
            .args([
                "init",
                "--non-interactive",
                "--language",
                "english",
                "--timezone",
                "UTC",
            ])
            .output()
            .expect("spawn fluxmirror init");
        assert!(
            output.status.success(),
            "init exit non-zero: stderr={}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    assert_eq!(
        count_setup_rows(&db),
        1,
        "two inits should still yield exactly one demo row"
    );
}

#[test]
fn no_demo_row_opts_out() {
    let (home, db) = fresh_sandbox("optout");
    let output = fluxmirror_bin()
        .env("HOME", home.path())
        .env_remove("USERPROFILE")
        .env("FLUXMIRROR_DB", &db)
        .args([
            "init",
            "--non-interactive",
            "--no-demo-row",
            "--language",
            "english",
            "--timezone",
            "UTC",
        ])
        .output()
        .expect("spawn fluxmirror init");

    assert!(
        output.status.success(),
        "init exit non-zero: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    // The DB may not exist at all when --no-demo-row skips the insert
    // (init does not eagerly create the events.db). If it doesn't
    // exist, that itself proves zero rows. Otherwise: assert zero.
    if db.exists() {
        assert_eq!(
            count_setup_rows(&db),
            0,
            "--no-demo-row should skip the insert"
        );
    }
}
