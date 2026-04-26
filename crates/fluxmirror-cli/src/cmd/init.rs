// fluxmirror init — first-run wizard.
//
// Tier A questions (always asked unless --non-interactive): language, timezone.
// Tier B questions (only when --advanced): self-noise, retention, agents.
//
// On completion:
//   * `${config_dir()}/config.json` is written atomically with schema_version=1
//   * The chosen wrapper is applied via `wrapper::apply_set`
//   * A compressed `welcome.md` is dropped under `config_dir()`
//   * Unless `--no-demo-row` is passed, one synthetic `agent='setup'`
//     row is inserted into `agent_events` so `/fluxmirror:today` returns
//     a meaningful report immediately on a fresh DB.
//   * A summary block is printed to stdout
//
// All prompts are plain stdin readline; no extra deps.

use std::env;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use chrono::{SecondsFormat, Utc};
use fluxmirror_core::report::lang::pack as lang_pack;
use fluxmirror_core::{
    chrono_tz::Tz, paths, tz, AgentEvent, AgentId, AgentToggle, AgentsConfig, Config, Language,
    SelfNoiseConfig, StorageConfig, ToolClass, ToolKind, WrapperConfig, WrapperKind,
};
use fluxmirror_store::{EventStore, SqliteStore};

use crate::cmd::wrapper::{self, EngineInfo};

pub fn run(
    advanced: bool,
    non_interactive: bool,
    language: Option<String>,
    timezone: Option<String>,
    demo_row: bool,
) -> ExitCode {
    // 1. Resolve language.
    let lang = match language.as_deref() {
        Some(s) => match parse_language(s) {
            Some(l) => l,
            None => {
                eprintln!("fluxmirror init: invalid --language {:?} (expected: english | korean | japanese | chinese)", s);
                return ExitCode::from(2);
            }
        },
        None => infer_language(),
    };

    // 2. Resolve timezone.
    let tz_name = match timezone.as_deref() {
        Some(s) => match s.parse::<Tz>() {
            Ok(t) => t.name().to_string(),
            Err(_) => {
                eprintln!("fluxmirror init: invalid --timezone {:?}", s);
                return ExitCode::from(2);
            }
        },
        None => tz::infer_default_tz().name().to_string(),
    };

    // 3. Tier A prompts (skip if --non-interactive).
    let stdin = io::stdin();
    let mut handle = stdin.lock();

    let lang = if !non_interactive {
        ask_language(&mut handle, lang)
    } else {
        lang
    };
    let tz_name = if !non_interactive {
        match ask_timezone(&mut handle, &tz_name) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("fluxmirror init: {e}");
                return ExitCode::from(2);
            }
        }
    } else {
        tz_name
    };

    // 4. Tier B prompts (only when --advanced AND not --non-interactive).
    let mut self_noise = SelfNoiseConfig::default();
    let mut retention_days: Option<u32> = None;
    let mut agents = AgentsConfig::default();
    if advanced && !non_interactive {
        self_noise.enabled = ask_yes_no(&mut handle, "Track FluxMirror repo activity (self-noise)?", false);
        retention_days = ask_optional_u32(&mut handle, "Retention days (blank = forever)?");
        agents.claude_code = AgentToggle {
            enabled: ask_yes_no(&mut handle, "Enable agent claude-code?", true),
        };
        agents.qwen_code = AgentToggle {
            enabled: ask_yes_no(&mut handle, "Enable agent qwen-code?", true),
        };
        agents.gemini_cli = AgentToggle {
            enabled: ask_yes_no(&mut handle, "Enable agent gemini-cli?", true),
        };
        agents.claude_desktop = AgentToggle {
            enabled: ask_yes_no(&mut handle, "Enable agent claude-desktop?", true),
        };
    }

    // 5. Probe wrappers and pick recommended.
    let engines = wrapper::probe_engines();
    let recommended = recommend_wrapper(&engines);
    let chosen_wrapper = match recommended {
        Some(k) => {
            // Multiple viable + interactive → prompt; else silent auto-pick.
            let viable: Vec<&str> = engines
                .iter()
                .filter(|e| e.available && wrapper_kind_for_engine(e.name).is_some())
                .map(|e| e.name)
                .collect();
            if !non_interactive && viable.len() > 1 {
                ask_wrapper(&mut handle, &viable, k)
            } else {
                k.to_string()
            }
        }
        None => {
            eprintln!(
                "fluxmirror init: no usable wrapper engine detected.\n  Install one of:\n    bash (POSIX shell — usually pre-installed on macOS / Linux / WSL)\n    node (https://nodejs.org)\n  On Windows you can also use:\n    cmd  (built-in)"
            );
            return ExitCode::from(2);
        }
    };

    // 6. Build Config and persist.
    let mut cfg = Config::default();
    cfg.schema_version = 1;
    cfg.language = lang;
    cfg.timezone = tz_name.clone();
    cfg.storage = StorageConfig {
        kind: "sqlite".into(),
        path: None,
        retention_days,
    };
    cfg.self_noise = self_noise;
    cfg.agents = agents;
    cfg.wrapper = WrapperConfig {
        kind: wrapper_kind_from_str(&chosen_wrapper),
        path: None,
        selected_at: None,
        auto_detected: !non_interactive
            || (timezone.is_none() && language.is_none()),
        ..Default::default()
    };

    let cfg_path = paths::config_dir().join("config.json");
    if let Err(e) = save_config_atomic(&cfg, &cfg_path) {
        eprintln!("fluxmirror init: failed to write {}: {e}", cfg_path.display());
        return ExitCode::from(2);
    }

    // 7. Apply wrapper choice (rewrites hooks.json under installed plugins).
    //    Only attempt if at least one of the well-known plugin install
    //    dirs exists. Otherwise emit a hint.
    let plugin_present = installed_plugin_present();
    if plugin_present {
        // Apply silently — wrapper::apply_set prints its own status lines.
        let _ = wrapper::apply_set(&chosen_wrapper);
    } else if non_interactive {
        println!(
            "no plugin install detected; run `fluxmirror wrapper set {}` later",
            chosen_wrapper
        );
    }

    // 8. Drop the compressed welcome.md into the config dir. Errors here
    //    are swallowed — first-run friendliness must never block init.
    let _ = write_welcome_md(&paths::config_dir());

    // 9. Demo row insert. Default-on so a fresh user gets a meaningful
    //    /fluxmirror:today report immediately. Errors are reported as a
    //    warning to stderr and never break init (NF-3).
    let db_path = cfg.effective_db_path();
    if demo_row {
        match insert_demo_row(&db_path) {
            Ok(true) => {
                println!("{}", lang_pack(lang.as_str()).init_demo_row_inserted);
            }
            Ok(false) => {
                // Already present — idempotent re-run, stay quiet.
            }
            Err(e) => {
                eprintln!("fluxmirror init: demo row insert failed: {e}");
            }
        }
    }

    // 10. Summary.
    println!();
    println!("Wrote config: {}", cfg_path.display());
    println!("Language:    {}", lang.as_str());
    println!("Timezone:    {}", tz_name);
    println!("Wrapper:     {}", chosen_wrapper);
    println!("DB path:     {}", db_path.display());
    println!();
    println!("Next: try `fluxmirror today` from a Claude Code / Qwen Code / Gemini CLI session.");

    ExitCode::SUCCESS
}

/// Insert a single synthetic `agent='setup'` row into `agent_events`
/// so the very first invocation of `/fluxmirror:today` returns a
/// non-empty report. Idempotent: if a row with `agent='setup'` and
/// `session='init-demo'` already exists, the function returns
/// `Ok(false)` and inserts nothing.
///
/// All errors bubble up to the caller, which is responsible for
/// converting them into a warning — init must never fail because the
/// demo row could not be written (NF-3: telemetry must never break the
/// user's CLI).
fn insert_demo_row(db_path: &Path) -> Result<bool, String> {
    let store = SqliteStore::open(db_path).map_err(|e| format!("open db: {e}"))?;

    // Idempotency probe: another `init` run already left a demo row.
    if demo_row_exists(db_path)? {
        return Ok(false);
    }

    let cwd = std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("/"));
    let host = gethostname::gethostname()
        .to_string_lossy()
        .into_owned();
    let user = env::var("USER")
        .or_else(|_| env::var("USERNAME"))
        .unwrap_or_default();
    let now = Utc::now();
    let ts_iso = now.to_rfc3339_opts(SecondsFormat::Secs, true);

    let detail = "fluxmirror init demo row \u{2014} delete me with: fluxmirror sqlite \"DELETE FROM agent_events WHERE agent='setup'\"".to_string();

    let raw_json = serde_json::json!({
        "ts": ts_iso,
        "agent": "setup",
        "session": "init-demo",
        "tool": "Init",
        "tool_canonical": "Init",
        "tool_class": "Meta",
        "detail": detail,
        "cwd": cwd.to_string_lossy(),
        "host": host,
        "user": user,
        "schema_version": 1,
    })
    .to_string();

    let event = AgentEvent {
        ts_utc: now,
        schema_version: 1,
        agent: AgentId::Other("setup".to_string()),
        session: "init-demo".to_string(),
        tool_raw: "Init".to_string(),
        tool_canonical: ToolKind::Other("Init".to_string()),
        tool_class: ToolClass::Meta,
        detail,
        cwd,
        host,
        user,
        raw_json,
    };

    store
        .write_agent_event(&event)
        .map_err(|e| format!("write demo row: {e}"))?;
    Ok(true)
}

/// Probe the DB for an existing `agent='setup' AND session='init-demo'`
/// row. Returns `Ok(false)` if the row is missing OR the table does not
/// yet exist (the SqliteStore::open call will create it next).
fn demo_row_exists(db_path: &Path) -> Result<bool, String> {
    use rusqlite::OpenFlags;
    if !db_path.exists() {
        return Ok(false);
    }
    let conn = rusqlite::Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| format!("open db: {e}"))?;
    // Table may not exist on a brand-new DB that we are about to open
    // through SqliteStore — treat that as "no row".
    let exists: i64 = match conn.query_row(
        "SELECT COUNT(*) FROM agent_events WHERE agent = 'setup' AND session = 'init-demo'",
        [],
        |r| r.get(0),
    ) {
        Ok(n) => n,
        Err(_) => return Ok(false),
    };
    Ok(exists > 0)
}

/// Drop a tightly compressed welcome.md into the config dir. The
/// content is intentionally small: a tagline, three try-first slash
/// commands, a paragraph on data location, and an asciinema embed
/// placeholder. Total target: ≤ 25 lines.
fn write_welcome_md(dir: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    let welcome = dir.join("welcome.md");
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
    fs::write(welcome, body)
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn parse_language(s: &str) -> Option<Language> {
    match s.to_ascii_lowercase().as_str() {
        "english" | "en" | "en_us" => Some(Language::English),
        "korean" | "ko" | "kr" | "ko_kr" => Some(Language::Korean),
        "japanese" | "ja" | "ja_jp" => Some(Language::Japanese),
        "chinese" | "zh" | "zh_cn" => Some(Language::Chinese),
        _ => None,
    }
}

fn infer_language() -> Language {
    for key in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(v) = env::var(key) {
            if !v.is_empty() {
                return Language::from_locale(&v);
            }
        }
    }
    Language::English
}

fn ask_language<R: BufRead>(input: &mut R, default: Language) -> Language {
    let prompt = format!(
        "Preferred report language [english/korean/japanese/chinese] (default: {})> ",
        default.as_str()
    );
    print_prompt(&prompt);
    let line = read_line(input);
    let trimmed = line.trim();
    if trimmed.is_empty() {
        default
    } else {
        parse_language(trimmed).unwrap_or(default)
    }
}

fn ask_timezone<R: BufRead>(input: &mut R, default: &str) -> Result<String, String> {
    let prompt = format!("Timezone (IANA, default: {})> ", default);
    print_prompt(&prompt);
    let line = read_line(input);
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Ok(default.to_string());
    }
    match trimmed.parse::<Tz>() {
        Ok(t) => Ok(t.name().to_string()),
        Err(_) => Err(format!("invalid timezone {:?}", trimmed)),
    }
}

fn ask_yes_no<R: BufRead>(input: &mut R, q: &str, default_yes: bool) -> bool {
    let suffix = if default_yes { "(Y/n)" } else { "(y/N)" };
    print_prompt(&format!("{q} {suffix}> "));
    let line = read_line(input);
    match line.trim().to_ascii_lowercase().as_str() {
        "" => default_yes,
        "y" | "yes" => true,
        "n" | "no" => false,
        _ => default_yes,
    }
}

fn ask_optional_u32<R: BufRead>(input: &mut R, q: &str) -> Option<u32> {
    print_prompt(&format!("{q} > "));
    let line = read_line(input);
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    trimmed.parse::<u32>().ok()
}

fn ask_wrapper<R: BufRead>(input: &mut R, viable: &[&str], default: &str) -> String {
    let prompt = format!(
        "Wrapper engine [{}] (default: {})> ",
        viable.join("/"),
        default
    );
    print_prompt(&prompt);
    let line = read_line(input);
    let trimmed = line.trim().to_ascii_lowercase();
    if trimmed.is_empty() {
        return default.to_string();
    }
    let cleaned = trimmed.as_str();
    if viable.iter().any(|v| *v == cleaned) {
        cleaned.to_string()
    } else {
        default.to_string()
    }
}

fn print_prompt(s: &str) {
    print!("{s}");
    let _ = io::stdout().flush();
}

fn read_line<R: BufRead>(input: &mut R) -> String {
    let mut buf = String::new();
    let _ = input.read_line(&mut buf);
    buf
}

/// Best-effort recommended engine name (matches a wrapper kind):
///   * macOS / Linux: bash > node > error
///   * Windows: cmd > node > error
fn recommend_wrapper(engines: &[EngineInfo]) -> Option<&'static str> {
    let avail = |name: &str| -> bool {
        engines
            .iter()
            .any(|e| e.name == name && e.available)
    };
    if cfg!(target_os = "windows") {
        if avail("cmd") {
            return Some("cmd");
        }
        if avail("node") {
            return Some("node");
        }
        return None;
    }
    if avail("bash") {
        return Some("bash");
    }
    if avail("node") {
        return Some("node");
    }
    None
}

fn wrapper_kind_for_engine(name: &str) -> Option<&'static str> {
    match name {
        "bash" => Some("bash"),
        "node" => Some("node"),
        "cmd" => Some("cmd"),
        _ => None,
    }
}

fn wrapper_kind_from_str(s: &str) -> WrapperKind {
    match s {
        "bash" => WrapperKind::Bash,
        "node" => WrapperKind::Node,
        "cmd" => WrapperKind::Cmd,
        "pwsh" => WrapperKind::Pwsh,
        _ => WrapperKind::Auto,
    }
}

fn save_config_atomic(cfg: &Config, path: &PathBuf) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let s = serde_json::to_string_pretty(cfg)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, s)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn installed_plugin_present() -> bool {
    let Some(home) = paths::home_dir() else {
        return false;
    };
    let candidates = [
        home.join(".claude/plugins/fluxmirror"),
        home.join(".qwen/plugins/fluxmirror"),
        home.join(".gemini/extensions/fluxmirror"),
    ];
    candidates.iter().any(|p| p.exists())
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmd::util::test_helpers::{env_lock, EnvGuard};

    fn read_config(path: &PathBuf) -> serde_json::Value {
        let bytes = std::fs::read(path).expect("config.json should exist");
        serde_json::from_slice(&bytes).expect("config.json should parse")
    }

    #[test]
    fn parse_language_table() {
        assert_eq!(parse_language("english"), Some(Language::English));
        assert_eq!(parse_language("KO"), Some(Language::Korean));
        assert_eq!(parse_language("japanese"), Some(Language::Japanese));
        assert_eq!(parse_language("zh"), Some(Language::Chinese));
        assert_eq!(parse_language("klingon"), None);
    }

    #[test]
    fn non_interactive_writes_config() {
        let _lock = env_lock();
        let tmp = tempfile::tempdir().unwrap();
        let _h = EnvGuard::set("HOME", tmp.path().to_str().unwrap());
        let _u = EnvGuard::unset("USERPROFILE");
        let _l = EnvGuard::unset("LANG");
        let _lc = EnvGuard::unset("LC_ALL");
        let _lm = EnvGuard::unset("LC_MESSAGES");
        let _fl = EnvGuard::unset("FLUXMIRROR_LANGUAGE");
        let _ft = EnvGuard::unset("FLUXMIRROR_TIMEZONE");
        let _fd = EnvGuard::unset("FLUXMIRROR_DB");

        let code = run(
            false,
            true,
            Some("korean".into()),
            Some("Asia/Seoul".into()),
            false,
        );
        assert_eq!(format!("{code:?}"), format!("{:?}", ExitCode::SUCCESS));

        let cfg_path = paths::config_dir().join("config.json");
        let v = read_config(&cfg_path);
        assert_eq!(v["language"], "korean");
        assert_eq!(v["timezone"], "Asia/Seoul");
        assert_eq!(v["schema_version"], 1);
    }

    #[test]
    fn non_interactive_uses_defaults_when_no_flags() {
        let _lock = env_lock();
        let tmp = tempfile::tempdir().unwrap();
        let _h = EnvGuard::set("HOME", tmp.path().to_str().unwrap());
        let _u = EnvGuard::unset("USERPROFILE");
        let _l = EnvGuard::set("LANG", "ja_JP.UTF-8");
        let _lc = EnvGuard::unset("LC_ALL");
        let _lm = EnvGuard::unset("LC_MESSAGES");

        let code = run(false, true, None, None, false);
        assert_eq!(format!("{code:?}"), format!("{:?}", ExitCode::SUCCESS));

        let cfg_path = paths::config_dir().join("config.json");
        let v = read_config(&cfg_path);
        // LANG=ja_JP.UTF-8 → japanese.
        assert_eq!(v["language"], "japanese");
        // Timezone is whatever the host inferred; just verify it's a non-empty string.
        assert!(v["timezone"].as_str().map(|s| !s.is_empty()).unwrap_or(false));
    }

    #[test]
    fn init_invalid_language_exits_2() {
        let _lock = env_lock();
        let tmp = tempfile::tempdir().unwrap();
        let _h = EnvGuard::set("HOME", tmp.path().to_str().unwrap());
        let _u = EnvGuard::unset("USERPROFILE");

        let code = run(false, true, Some("klingon".into()), None, false);
        assert_eq!(format!("{code:?}"), format!("{:?}", ExitCode::from(2)));
    }

    #[test]
    fn init_invalid_timezone_exits_2() {
        let _lock = env_lock();
        let tmp = tempfile::tempdir().unwrap();
        let _h = EnvGuard::set("HOME", tmp.path().to_str().unwrap());
        let _u = EnvGuard::unset("USERPROFILE");

        let code = run(false, true, None, Some("Atlantis/Lost".into()), false);
        assert_eq!(format!("{code:?}"), format!("{:?}", ExitCode::from(2)));
    }

    #[test]
    fn recommend_wrapper_prefers_bash_on_posix() {
        let engines = vec![
            EngineInfo { name: "bash", available: true, path: Some("/bin/bash".into()) },
            EngineInfo { name: "node", available: true, path: Some("/usr/bin/node".into()) },
            EngineInfo { name: "pwsh", available: false, path: None },
            EngineInfo { name: "cmd", available: false, path: None },
        ];
        // On non-Windows, we expect "bash" first.
        if !cfg!(target_os = "windows") {
            assert_eq!(recommend_wrapper(&engines), Some("bash"));
        }
    }
}
