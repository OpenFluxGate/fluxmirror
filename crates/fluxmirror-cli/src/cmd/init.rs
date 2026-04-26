// fluxmirror init — first-run wizard.
//
// Tier A questions (always asked unless --non-interactive): language, timezone.
// Tier B questions (only when --advanced): self-noise, retention, agents.
//
// On completion:
//   * `${config_dir()}/config.json` is written atomically with schema_version=1
//   * The chosen wrapper is applied via `wrapper::apply_set`
//   * A summary block is printed to stdout
//
// All prompts are plain stdin readline; no extra deps.

use std::env;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use fluxmirror_core::{
    chrono_tz::Tz, paths, tz, AgentToggle, AgentsConfig, Config, Language, SelfNoiseConfig,
    StorageConfig, WrapperConfig, WrapperKind,
};

use crate::cmd::wrapper::{self, EngineInfo};

pub fn run(
    advanced: bool,
    non_interactive: bool,
    language: Option<String>,
    timezone: Option<String>,
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

    // 8. Summary.
    let db_path = cfg.effective_db_path();
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

        let code = run(false, true, None, None);
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

        let code = run(false, true, Some("klingon".into()), None);
        assert_eq!(format!("{code:?}"), format!("{:?}", ExitCode::from(2)));
    }

    #[test]
    fn init_invalid_timezone_exits_2() {
        let _lock = env_lock();
        let tmp = tempfile::tempdir().unwrap();
        let _h = EnvGuard::set("HOME", tmp.path().to_str().unwrap());
        let _u = EnvGuard::unset("USERPROFILE");

        let code = run(false, true, None, Some("Atlantis/Lost".into()));
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
