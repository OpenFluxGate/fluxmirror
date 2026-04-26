// fluxmirror hook — single-binary tool-call hook.
//
// Reads a tool-call JSON payload on stdin, writes one JSONL line to
// ~/<agent>/session-logs/YYYY-MM-DD.jsonl and one parameter-bound row
// to the FluxMirror SQLite database.
//
// Designed to never break the calling agent: every IO error is logged
// to ~/.fluxmirror/hook-errors.log (with 5 MiB rotation) and swallowed.
// Process exit code is always 0.
//
// CLI:
//   fluxmirror hook                   # auto-detect kind from env
//   fluxmirror hook --kind claude     # claude or qwen (resolved via env)
//   fluxmirror hook --kind gemini     # always gemini-cli
//
// Env vars:
//   FLUXMIRROR_DB         override DB path
//   FLUXMIRROR_SKIP_SELF  if "1" + FLUXMIRROR_SELF_REPO set, skip self-noise
//   FLUXMIRROR_SELF_REPO  absolute path to fluxmirror repo
//   QWEN_CODE_NO_RELAUNCH set when running under Qwen Code
//   QWEN_PROJECT_DIR      set when running under Qwen Code
//
// STEP 3 note: SQLite writes go through `fluxmirror_store::SqliteStore`.
// The store owns schema creation + additive migration of legacy DBs;
// this module now only builds the canonical `AgentEvent` and hands it
// off. Errors at any stage still funnel through `log_error` and the
// process exits 0 — the hook must never break the calling agent.

use chrono::{DateTime, SecondsFormat, Utc};
use fluxmirror_core::{extract_detail, normalize, paths, AgentEvent, AgentId};
use fluxmirror_store::{EventStore, SqliteStore};
use serde_json::Value;
use std::env;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const ERR_LOG_MAX_BYTES: u64 = 5 * 1024 * 1024;
const SCHEMA_VERSION: u32 = 1;

/// Public entry point invoked by the CLI dispatcher. Always exits 0 —
/// telemetry must never kill the calling agent's tool call.
pub fn run(argv: Vec<String>) -> ExitCode {
    let _ = run_inner(&argv);
    ExitCode::SUCCESS
}

#[derive(Debug, Clone, Copy)]
enum Kind {
    Claude,
    Gemini,
}

fn run_inner(argv: &[String]) -> Result<(), String> {
    let kind = parse_kind(argv);

    // Read stdin
    let mut input = String::new();
    io::stdin()
        .read_to_string(&mut input)
        .map_err(|e| format!("read stdin: {e}"))?;

    // Parse JSON
    let v: Value = serde_json::from_str(&input).map_err(|e| {
        log_error(&format!(
            "invalid JSON ({e}); first 120 bytes: {:?}",
            input.chars().take(120).collect::<String>()
        ));
        format!("parse: {e}")
    })?;

    let tool_raw = match v.get("tool_name").and_then(|s| s.as_str()) {
        Some(t) if !t.is_empty() => t.to_string(),
        _ => return Ok(()), // missing tool_name → silently no-op (matches bash behavior)
    };
    let session = v
        .get("session_id")
        .and_then(|s| s.as_str())
        .unwrap_or("unknown")
        .to_string();
    let cwd_str = v
        .get("cwd")
        .and_then(|s| s.as_str())
        .unwrap_or("unknown")
        .to_string();

    let (tool_canonical, tool_class) = normalize(&tool_raw);
    let detail = extract_detail(&tool_canonical, v.get("tool_input"));

    // Resolve agent label + log base
    let (agent, log_base) = resolve_agent(kind);

    // Self-noise filter
    if should_skip_self_noise(&tool_raw, &cwd_str, &detail) {
        return Ok(());
    }

    // Build the canonical event.
    let event = AgentEvent {
        ts_utc: Utc::now(),
        schema_version: SCHEMA_VERSION,
        agent: AgentId::from_str(&agent),
        session: session.clone(),
        tool_raw: tool_raw.clone(),
        tool_canonical,
        tool_class,
        detail: detail.clone(),
        cwd: PathBuf::from(&cwd_str),
        host: gethostname::gethostname().to_string_lossy().into_owned(),
        user: env::var("USER")
            .or_else(|_| env::var("USERNAME"))
            .unwrap_or_default(),
        raw_json: input.clone(),
    };

    // JSONL append (best-effort)
    if let Err(e) = append_jsonl(&log_base, &event) {
        log_error(&format!("jsonl append: {e}"));
    }

    // SQLite write (best-effort, with full error logging)
    let db_path = paths::default_db_path();
    match SqliteStore::open(&db_path) {
        Ok(store) => match store.write_agent_event(&event) {
            Ok(()) => {
                // First-fire onboarding: write a welcome.md + a marker
                // file so subsequent fires no-op. Best-effort only.
                write_welcome_once(&db_path);
            }
            Err(e) => {
                log_error(&format!(
                    "sqlite write (agent={} tool={}): {e}",
                    event.agent.as_str(),
                    event.tool_raw,
                ));
            }
        },
        Err(e) => log_error(&format!(
            "sqlite open ({}): {e}",
            db_path.display()
        )),
    }

    Ok(())
}

/// On the first successful event-write, drop a `.first-fire-at` marker
/// (and a compressed `welcome.md`, if init has not already emitted one)
/// so the user has a friendly local landing page for the new tool.
/// Idempotent: returns silently if the marker exists. Every IO error
/// is swallowed — the hook must always exit 0.
fn write_welcome_once(_db_path: &Path) {
    let dir = paths::config_dir();
    let marker = dir.join(".first-fire-at");
    if marker.exists() {
        return;
    }
    let _ = fs::create_dir_all(&dir);
    let ts = format_iso8601(&Utc::now());
    let _ = fs::write(&marker, ts.as_bytes());

    let welcome = dir.join("welcome.md");
    if welcome.exists() {
        // `fluxmirror init` already wrote the canonical compressed
        // welcome page — don't clobber it on first hook fire.
        return;
    }
    let body = "# FluxMirror

FluxMirror is now logging your AI agent activity locally.

## Try these first

- /fluxmirror:today
- /fluxmirror:agents
- /fluxmirror:doctor

## Where data lives

Your activity is recorded in a single SQLite database under the
fluxmirror data dir for your OS (`~/Library/Application Support/fluxmirror/`
on macOS, `${XDG_DATA_HOME:-~/.local/share}/fluxmirror/` on Linux,
`%APPDATA%\\fluxmirror\\` on Windows). Run `fluxmirror doctor` for a
five-component health check, or read the project README for the full
configuration / migration story.

<!-- ASCIINEMA_PLACEHOLDER -->
";
    let _ = fs::write(&welcome, body.as_bytes());
}

fn parse_kind(args: &[String]) -> Kind {
    // Tiny CLI: look for --kind <value>. Argv here is post-subcommand,
    // so we do NOT skip the program name (the dispatcher already did).
    let mut iter = args.iter();
    while let Some(a) = iter.next() {
        if a == "--kind" {
            if let Some(v) = iter.next() {
                match v.as_str() {
                    "gemini" => return Kind::Gemini,
                    _ => return Kind::Claude,
                }
            }
        }
    }
    Kind::Claude
}

fn resolve_agent(kind: Kind) -> (String, PathBuf) {
    let home = paths::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
    match kind {
        Kind::Gemini => ("gemini-cli".to_string(), home.join(".gemini")),
        Kind::Claude => {
            // Qwen reuses the Claude plugin. Detect via env signals.
            if env::var_os("QWEN_CODE_NO_RELAUNCH").is_some()
                || env::var_os("QWEN_PROJECT_DIR").is_some()
            {
                ("qwen-code".to_string(), home.join(".qwen"))
            } else {
                ("claude-code".to_string(), home.join(".claude"))
            }
        }
    }
}

fn should_skip_self_noise(tool: &str, cwd: &str, detail: &str) -> bool {
    if env::var("FLUXMIRROR_SKIP_SELF").as_deref() != Ok("1") {
        return false;
    }
    let repo = match env::var("FLUXMIRROR_SELF_REPO") {
        Ok(v) if !v.is_empty() => v,
        _ => return false,
    };

    let is_shell = matches!(tool, "Bash" | "run_shell_command");
    if !is_shell {
        return false;
    }

    let cwd_real = canonical(cwd);
    let repo_real = canonical(&repo);
    if cwd_real.is_none() || repo_real.is_none() {
        return false;
    }
    let mut cwd_with_sep = cwd_real.unwrap().to_string_lossy().into_owned();
    cwd_with_sep.push('/');
    let mut repo_with_sep = repo_real.unwrap().to_string_lossy().into_owned();
    repo_with_sep.push('/');

    if !cwd_with_sep.starts_with(&repo_with_sep) {
        return false;
    }

    // detail looks like a fluxmirror DB query
    let lc = detail.to_ascii_lowercase();
    lc.contains("sqlite3") && lc.contains("events.db")
        || lc.contains("fluxmirror") && lc.contains(".db")
}

fn canonical<P: AsRef<Path>>(p: P) -> Option<PathBuf> {
    fs::canonicalize(p).ok()
}

fn append_jsonl(log_base: &Path, event: &AgentEvent) -> io::Result<()> {
    let dir = log_base.join("session-logs");
    fs::create_dir_all(&dir)?;
    let date = event.ts_utc.format("%Y-%m-%d").to_string();
    let file = dir.join(format!("{date}.jsonl"));

    let line = serde_json::json!({
        "ts": format_iso8601(&event.ts_utc),
        "session": event.session,
        "tool": event.tool_raw,
        "detail": event.detail,
        "cwd": event.cwd.to_string_lossy(),
    });
    let mut f = OpenOptions::new().create(true).append(true).open(&file)?;
    writeln!(f, "{}", line)?;
    Ok(())
}

fn format_iso8601(t: &DateTime<Utc>) -> String {
    // Second precision, trailing Z — matches the legacy hand-rolled formatter.
    t.to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn log_error(msg: &str) {
    let Some(home) = paths::home_dir() else { return };
    let dir = home.join(".fluxmirror");
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    let log = dir.join("hook-errors.log");
    let _ = rotate_if_needed(&log);
    let ts = format_iso8601(&Utc::now());
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&log) {
        let _ = writeln!(f, "[{ts}] {msg}");
    }
}

fn rotate_if_needed(log: &Path) -> io::Result<()> {
    let meta = match fs::metadata(log) {
        Ok(m) => m,
        Err(_) => return Ok(()), // doesn't exist yet
    };
    if meta.len() < ERR_LOG_MAX_BYTES {
        return Ok(());
    }
    let backup = log.with_extension("log.1");
    fs::rename(log, &backup)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_kind_defaults_to_claude() {
        let args: Vec<String> = vec![];
        assert!(matches!(parse_kind(&args), Kind::Claude));
    }

    #[test]
    fn parse_kind_explicit_gemini() {
        let args = vec!["--kind".to_string(), "gemini".to_string()];
        assert!(matches!(parse_kind(&args), Kind::Gemini));
    }

    #[test]
    fn parse_kind_explicit_claude() {
        let args = vec!["--kind".to_string(), "claude".to_string()];
        assert!(matches!(parse_kind(&args), Kind::Claude));
    }

    #[test]
    fn iso8601_seconds_precision_with_z() {
        let t: DateTime<Utc> = "2026-05-01T23:00:00Z".parse().unwrap();
        assert_eq!(format_iso8601(&t), "2026-05-01T23:00:00Z");
    }

    #[test]
    fn _kind_variants_used() {
        // Touch each variant so the dead-code lint stays quiet without a
        // crate-wide allow.
        let _g: Kind = Kind::Gemini;
        let _c: Kind = Kind::Claude;
        // Sanity-check that the core normalize call works through this crate.
        let (k, c) = normalize("Bash");
        assert_eq!(c.as_str(), "Shell");
        assert_eq!(k.as_str(), "Bash");
    }

    use crate::cmd::util::test_helpers::{env_lock, EnvGuard};

    #[test]
    fn first_fire_writes_marker_and_welcome() {
        let _lock = env_lock();
        let tmp = tempfile::tempdir().unwrap();
        let _h = EnvGuard::set("HOME", tmp.path().to_str().unwrap());
        let _u = EnvGuard::unset("USERPROFILE");

        let db_path = tmp.path().join("events.db");
        write_welcome_once(&db_path);

        let cfg_dir = paths::config_dir();
        assert!(cfg_dir.join(".first-fire-at").exists());
        assert!(cfg_dir.join("welcome.md").exists());

        let body = fs::read_to_string(cfg_dir.join("welcome.md")).unwrap();
        // Compressed welcome (≤ 25 lines): tagline + 3 try-first commands
        // + where-data-lives paragraph + asciinema placeholder.
        assert!(body.contains("FluxMirror"));
        assert!(body.contains("/fluxmirror:today"));
        assert!(body.contains("/fluxmirror:agents"));
        assert!(body.contains("/fluxmirror:doctor"));
        assert!(body.contains("ASCIINEMA_PLACEHOLDER"));
        let line_count = body.lines().count();
        assert!(
            line_count <= 25,
            "welcome.md should be ≤ 25 lines, got {line_count}"
        );
    }

    #[test]
    fn second_fire_does_not_overwrite() {
        let _lock = env_lock();
        let tmp = tempfile::tempdir().unwrap();
        let _h = EnvGuard::set("HOME", tmp.path().to_str().unwrap());
        let _u = EnvGuard::unset("USERPROFILE");

        let db_path = tmp.path().join("events.db");
        write_welcome_once(&db_path);

        let cfg_dir = paths::config_dir();
        let marker = cfg_dir.join(".first-fire-at");
        let m1 = fs::metadata(&marker).unwrap();
        let mtime1 = m1.modified().unwrap();
        let body1 = fs::read_to_string(cfg_dir.join("welcome.md")).unwrap();

        // Sleep 1.1s to let the FS clock tick if it cared, then re-fire.
        // (The check here is "marker.exists() short-circuits", so even
        // sub-second precision is fine; this just makes the assertion
        // robust on filesystems with low-resolution mtime.)
        std::thread::sleep(std::time::Duration::from_millis(1100));
        write_welcome_once(&db_path);

        let m2 = fs::metadata(&marker).unwrap();
        let mtime2 = m2.modified().unwrap();
        let body2 = fs::read_to_string(cfg_dir.join("welcome.md")).unwrap();

        assert_eq!(mtime1, mtime2, "marker mtime must not change");
        assert_eq!(body1, body2, "welcome.md content must not change");
    }
}
