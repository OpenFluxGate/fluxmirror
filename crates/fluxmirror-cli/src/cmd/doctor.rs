// fluxmirror doctor — health check.
//
// Prints a fixed-width table summarising the live state of:
//   * config       (does config.json exist + parse?)
//   * database     (does the SQLite DB exist + open + report row count?)
//   * wrapper      (is wrapper.kind set + is that engine available?)
//   * agents       (per-agent: home dir present? last hook fire?)
//   * binary       (env-baked semver)
//
// Exit code:
//   0 — every row "ok"
//   1 — at least one row "warn"
//   2 — at least one row "error" / required missing
//
// Migration warning: on macOS, the legacy DB path matches the current
// default. On Linux, pre-Phase-1 hooks wrote to the macOS-style path
// even though we now default to XDG; if such a legacy DB exists, we
// add a WARN row.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use fluxmirror_core::{paths, Config};
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;

use crate::cmd::wrapper::probe_engines;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Ok,
    Warn,
    Error,
}

impl Status {
    fn as_str(&self) -> &'static str {
        match self {
            Status::Ok => "ok",
            Status::Warn => "warn",
            Status::Error => "error",
        }
    }
}

struct Row {
    component: String,
    status: Status,
    detail: String,
}

pub fn run() -> ExitCode {
    let mut rows: Vec<Row> = Vec::new();

    rows.push(check_config());
    rows.push(check_database());
    if let Some(extra) = check_legacy_db() {
        rows.push(extra);
    }
    rows.push(check_wrapper());
    rows.extend(check_agents());
    rows.push(check_binary());

    print_table(&rows);

    let any_error = rows.iter().any(|r| r.status == Status::Error);
    let any_warn = rows.iter().any(|r| r.status == Status::Warn);

    if any_error {
        ExitCode::from(2)
    } else if any_warn {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

// ---------------------------------------------------------------------------
// row builders
// ---------------------------------------------------------------------------

fn check_config() -> Row {
    let path = paths::config_dir().join("config.json");
    if !path.exists() {
        return Row {
            component: "config".into(),
            status: Status::Error,
            detail: format!("missing: {}", path.display()),
        };
    }
    match fs::read(&path) {
        Ok(bytes) => match serde_json::from_slice::<Value>(&bytes) {
            Ok(v) => {
                let sv = v
                    .get("schema_version")
                    .and_then(|n| n.as_u64())
                    .unwrap_or(0);
                Row {
                    component: "config".into(),
                    status: Status::Ok,
                    detail: format!("{} (schema v{sv})", path.display()),
                }
            }
            Err(e) => Row {
                component: "config".into(),
                status: Status::Error,
                detail: format!("corrupt: {} ({e})", path.display()),
            },
        },
        Err(e) => Row {
            component: "config".into(),
            status: Status::Error,
            detail: format!("read: {} ({e})", path.display()),
        },
    }
}

fn check_database() -> Row {
    // Effective DB path honours FLUXMIRROR_DB and config.storage.path.
    let db_path = match Config::load() {
        Ok(c) => c.effective_db_path(),
        Err(_) => paths::default_db_path(),
    };
    if !db_path.exists() {
        return Row {
            component: "database".into(),
            status: Status::Error,
            detail: format!("missing: {}", db_path.display()),
        };
    }
    match Connection::open_with_flags(
        &db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) {
        Ok(conn) => {
            let rows: i64 = conn
                .query_row("SELECT COUNT(*) FROM agent_events", [], |r| r.get(0))
                .unwrap_or(0);
            let schema: i64 = conn
                .query_row(
                    "SELECT COALESCE(MAX(version), 0) FROM schema_meta",
                    [],
                    |r| r.get(0),
                )
                .unwrap_or(0);
            Row {
                component: "database".into(),
                status: Status::Ok,
                detail: format!(
                    "{} ({rows} rows, schema v{schema})",
                    db_path.display()
                ),
            }
        }
        Err(e) => Row {
            component: "database".into(),
            status: Status::Error,
            detail: format!("open {}: {e}", db_path.display()),
        },
    }
}

fn check_legacy_db() -> Option<Row> {
    // On macOS the "legacy" path equals the default path, so nothing to
    // report. On Linux, pre-Phase-1 hooks wrote to the macOS-style path
    // (~/Library/Application Support/...) and we now default to XDG.
    let legacy = paths::legacy_macos_db_path();
    let default = paths::default_db_path();
    if legacy == default {
        return None;
    }
    if !legacy.exists() {
        return None;
    }
    Some(Row {
        component: "legacy-db".into(),
        status: Status::Warn,
        detail: format!("found legacy DB: {} (consider migrating)", legacy.display()),
    })
}

fn check_wrapper() -> Row {
    let cfg_path = paths::config_dir().join("config.json");
    let kind = read_wrapper_kind(&cfg_path);
    let engines = probe_engines();
    let count_avail = engines.iter().filter(|e| e.available).count();
    let total = engines.len();

    match kind {
        Some(k) => {
            let info = engines.iter().find(|e| matches_engine(e.name, &k));
            let avail = info.map(|e| e.available).unwrap_or(false);
            if avail {
                let where_at = info
                    .and_then(|e| e.path.clone())
                    .unwrap_or_else(|| "<unresolved path>".into());
                Row {
                    component: "wrapper".into(),
                    status: Status::Ok,
                    detail: format!(
                        "{k} ({where_at}; {count_avail} of {total} engines available)"
                    ),
                }
            } else {
                Row {
                    component: "wrapper".into(),
                    status: Status::Warn,
                    detail: format!("{k} (engine not detected on host)"),
                }
            }
        }
        None => Row {
            component: "wrapper".into(),
            status: Status::Warn,
            detail: format!(
                "<unset> ({count_avail} of {total} engines available; run `fluxmirror init`)"
            ),
        },
    }
}

fn matches_engine(engine_name: &str, wrapper_kind: &str) -> bool {
    match wrapper_kind {
        "router" => engine_name == "bash",
        other => engine_name == other,
    }
}

fn check_agents() -> Vec<Row> {
    let home = paths::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
    let agents: &[(&str, &str)] = &[
        ("claude-code", ".claude"),
        ("qwen-code", ".qwen"),
        ("gemini-cli", ".gemini"),
        ("claude-desktop", ".claude-desktop"),
    ];

    // Open DB read-only once for last-fire lookups.
    let db_path = match Config::load() {
        Ok(c) => c.effective_db_path(),
        Err(_) => paths::default_db_path(),
    };
    let conn = Connection::open_with_flags(
        &db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok();

    let mut out = Vec::new();
    for (i, (agent, dir)) in agents.iter().enumerate() {
        let home_path = home.join(dir);
        let last = conn.as_ref().and_then(|c| last_fire_for(c, agent));
        let detail = match (home_path.exists(), last.as_deref()) {
            (true, Some(ts)) => format!(
                "{}: {} (last fire: {ts})",
                agent,
                home_path.display()
            ),
            (true, None) => format!(
                "{}: {} (no fires yet)",
                agent,
                home_path.display()
            ),
            (false, Some(ts)) => format!("{}: <not installed> (last fire: {ts})", agent),
            (false, None) => format!("{}: <not installed>", agent),
        };
        let status = if home_path.exists() || last.is_some() {
            Status::Ok
        } else {
            Status::Warn
        };
        out.push(Row {
            component: if i == 0 { "agents".into() } else { "".into() },
            status,
            detail,
        });
    }
    out
}

fn last_fire_for(conn: &Connection, agent: &str) -> Option<String> {
    conn.query_row(
        "SELECT MAX(ts) FROM agent_events WHERE agent = ?1",
        [agent],
        |r| r.get::<_, Option<String>>(0),
    )
    .ok()
    .flatten()
}

fn check_binary() -> Row {
    Row {
        component: "binary".into(),
        status: Status::Ok,
        detail: format!("version {}", env!("CARGO_PKG_VERSION")),
    }
}

fn read_wrapper_kind(path: &Path) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    let v: Value = serde_json::from_slice(&bytes).ok()?;
    v.get("wrapper")?
        .get("kind")?
        .as_str()
        .map(|s| s.to_string())
}

// ---------------------------------------------------------------------------
// table printing
// ---------------------------------------------------------------------------

fn print_table(rows: &[Row]) {
    // Fixed widths chosen so a typical row fits on an 80-col terminal but
    // long detail strings still wrap predictably (we just let them flow).
    println!("{:<18} {:<9} {}", "component", "status", "detail");
    for r in rows {
        println!(
            "{:<18} {:<9} {}",
            r.component,
            r.status.as_str(),
            r.detail
        );
    }
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmd::util::test_helpers::{env_lock, EnvGuard};
    use fluxmirror_store::SqliteStore;

    #[test]
    fn doctor_no_config_no_db() {
        let _lock = env_lock();
        let tmp = tempfile::tempdir().unwrap();
        let _h = EnvGuard::set("HOME", tmp.path().to_str().unwrap());
        let _u = EnvGuard::unset("USERPROFILE");
        let _fd = EnvGuard::unset("FLUXMIRROR_DB");

        let code = run();
        // Both config + database missing → exit 2.
        assert_eq!(format!("{code:?}"), format!("{:?}", ExitCode::from(2)));
    }

    #[test]
    fn doctor_with_config_and_db() {
        let _lock = env_lock();
        let tmp = tempfile::tempdir().unwrap();
        let _h = EnvGuard::set("HOME", tmp.path().to_str().unwrap());
        let _u = EnvGuard::unset("USERPROFILE");
        let _fd = EnvGuard::set(
            "FLUXMIRROR_DB",
            tmp.path().join("events.db").to_str().unwrap(),
        );

        // Seed config.json with a valid wrapper.
        let cfg_dir = paths::config_dir();
        std::fs::create_dir_all(&cfg_dir).unwrap();
        let cfg_path = cfg_dir.join("config.json");
        std::fs::write(
            &cfg_path,
            br#"{"schema_version":1,"language":"english","timezone":"UTC","wrapper":{"kind":"bash"}}"#,
        )
        .unwrap();

        // Seed DB via SqliteStore (creates schema_meta + agent_events).
        let store = SqliteStore::open(&tmp.path().join("events.db")).unwrap();
        drop(store);

        let code = run();
        // Config + db present; wrapper might warn if bash isn't available
        // but the test environment always has bash. Either way, no errors,
        // so exit must be 0 or 1 (not 2).
        let s = format!("{code:?}");
        assert!(
            s == format!("{:?}", ExitCode::SUCCESS) || s == format!("{:?}", ExitCode::from(1)),
            "expected ok/warn, got {s}"
        );
    }
}
