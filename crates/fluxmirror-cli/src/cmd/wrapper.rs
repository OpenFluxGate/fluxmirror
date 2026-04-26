// fluxmirror wrapper — wrapper engine selection.
//
// Three responsibilities:
//   * `show`  — print the engine recorded in user config + the resolved binary path.
//   * `probe` — sniff host capabilities (bash / node / pwsh / cmd) + shell context.
//   * `set`   — record engine choice in user config and rewrite every known
//                plugin's hooks.json to point at the matching shim path.

use clap::Subcommand;
use fluxmirror_core::paths;
use serde_json::{json, Value};
use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

#[derive(Subcommand)]
pub enum WrapperOp {
    /// Print the currently selected wrapper kind.
    Show,
    /// Probe the host environment and recommend a wrapper kind.
    Probe,
    /// Force a specific wrapper kind.
    Set { kind: String },
}

pub const VALID_KINDS: &[&str] = &["bash", "node", "cmd", "router"];

pub fn run(op: WrapperOp) -> ExitCode {
    match op {
        WrapperOp::Show => show(),
        WrapperOp::Probe => probe(),
        WrapperOp::Set { kind } => set(&kind),
    }
}

/// Re-exposed for callers (e.g. `init`) that need to apply a wrapper
/// choice without going through the full `WrapperOp::Set` dispatch.
pub fn apply_set(kind: &str) -> ExitCode {
    set(kind)
}

/// Snapshot of one engine's availability — used by `init` to pick a
/// recommended wrapper without re-implementing detection.
#[derive(Debug, Clone)]
pub struct EngineInfo {
    pub name: &'static str,
    pub available: bool,
    pub path: Option<String>,
}

/// Probe bash / node / pwsh / cmd availability. Pure: no stdout writes.
pub fn probe_engines() -> Vec<EngineInfo> {
    let mut out = Vec::new();
    for engine in ["bash", "node", "pwsh"] {
        let (avail, path) = which(engine);
        out.push(EngineInfo {
            name: engine,
            available: avail,
            path,
        });
    }
    let cmd_avail = cfg!(target_os = "windows") && which("cmd").0;
    out.push(EngineInfo {
        name: "cmd",
        available: cmd_avail,
        path: if cmd_avail {
            Some("%SystemRoot%\\System32\\cmd.exe".into())
        } else {
            None
        },
    });
    out
}

// ---------------------------------------------------------------------------
// show
// ---------------------------------------------------------------------------

fn show() -> ExitCode {
    let cfg_path = config_json_path();
    let kind = read_wrapper_kind(&cfg_path);
    match kind {
        Some(k) => {
            println!("engine: {k}");
            println!("binary: {}", shim_relpath(&k).unwrap_or_else(|| "default".into()));
        }
        None => {
            println!("engine: <not configured>");
            println!("binary: default");
        }
    }
    ExitCode::SUCCESS
}

// ---------------------------------------------------------------------------
// probe
// ---------------------------------------------------------------------------

fn probe() -> ExitCode {
    println!("engine\tavailable\tpath");

    for engine in ["bash", "node", "pwsh"] {
        let (avail, path) = which(engine);
        println!(
            "{}\t{}\t{}",
            engine,
            if avail { "yes" } else { "no" },
            path.unwrap_or_else(|| "-".into())
        );
    }

    // cmd only exists on Windows.
    let cmd_avail = cfg!(target_os = "windows") && which("cmd").0;
    println!(
        "cmd\t{}\t{}",
        if cmd_avail { "yes" } else { "no" },
        if cmd_avail { "%SystemRoot%\\System32\\cmd.exe" } else { "-" }
    );

    println!("context: {}", detect_context());
    ExitCode::SUCCESS
}

fn which(prog: &str) -> (bool, Option<String>) {
    // Prefer running the program itself rather than depending on `which(1)`.
    // We use `--version` (or `-Version` for pwsh) and `Stdio::null` everything.
    let arg = if prog == "pwsh" { "-Version" } else { "--version" };
    let status = Command::new(prog)
        .arg(arg)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    if !matches!(status, Ok(s) if s.success()) {
        return (false, None);
    }
    // Best-effort path resolution via PATH walk; falls back to `-`.
    let path = which_walk(prog);
    (true, path)
}

fn which_walk(prog: &str) -> Option<String> {
    let path_env = env::var_os("PATH")?;
    let exts: Vec<String> = if cfg!(target_os = "windows") {
        env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.CMD;.BAT".into())
            .split(';')
            .map(|s| s.to_string())
            .collect()
    } else {
        vec![String::new()]
    };
    for dir in env::split_paths(&path_env) {
        for ext in &exts {
            let candidate = dir.join(format!("{prog}{ext}"));
            if candidate.is_file() {
                return Some(candidate.to_string_lossy().into_owned());
            }
        }
    }
    None
}

fn detect_context() -> &'static str {
    if cfg!(target_os = "windows") {
        // Best-effort: cmd vs pwsh from PSModulePath presence.
        if env::var_os("PSModulePath").is_some() {
            return "windows-pwsh";
        }
        return "windows-cmd";
    }
    if env::var_os("WSL_DISTRO_NAME").is_some() {
        return "wsl";
    }
    if env::var_os("MSYSTEM").is_some() {
        return "mingw";
    }
    "posix"
}

// ---------------------------------------------------------------------------
// set
// ---------------------------------------------------------------------------

fn set(kind: &str) -> ExitCode {
    if !VALID_KINDS.contains(&kind) {
        eprintln!(
            "fluxmirror wrapper set: invalid kind {:?} (expected: {})",
            kind,
            VALID_KINDS.join(" | ")
        );
        return ExitCode::from(2);
    }

    // 1. Update user config atomically.
    let cfg_path = config_json_path();
    if let Err(e) = write_config_kind(&cfg_path, kind) {
        eprintln!("fluxmirror wrapper set: failed to update {}: {e}", cfg_path.display());
        return ExitCode::from(1);
    }
    println!("rewrote {} -> wrapper.kind={kind}", cfg_path.display());

    // 2. Rewrite every known hooks.json.
    let mut any_failed = false;
    for target in plugin_targets() {
        if !target.hooks_json.exists() {
            continue;
        }
        match rewrite_hooks_json(&target, kind) {
            Ok(true) => println!(
                "rewrote {} -> wrappers/{}",
                target.hooks_json.display(),
                shim_basename(kind).unwrap_or("router.sh")
            ),
            Ok(false) => {}
            Err(e) => {
                eprintln!("fluxmirror wrapper set: failed to rewrite {}: {e}", target.hooks_json.display());
                any_failed = true;
            }
        }
    }

    if any_failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

// ---------------------------------------------------------------------------
// config.json read/write
// ---------------------------------------------------------------------------

fn config_json_path() -> PathBuf {
    paths::config_dir().join("config.json")
}

fn read_wrapper_kind(path: &Path) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    let v: Value = serde_json::from_slice(&bytes).ok()?;
    v.get("wrapper")?
        .get("kind")?
        .as_str()
        .map(|s| s.to_string())
}

fn write_config_kind(path: &Path, kind: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    // Read-modify-write so we don't clobber unrelated keys.
    let mut value: Value = match fs::read(path) {
        Ok(b) if !b.is_empty() => serde_json::from_slice(&b).unwrap_or_else(|_| json!({})),
        _ => json!({}),
    };
    if !value.is_object() {
        value = json!({});
    }
    let obj = value.as_object_mut().unwrap();
    let wrapper = obj
        .entry("wrapper".to_string())
        .or_insert_with(|| json!({}));
    if !wrapper.is_object() {
        *wrapper = json!({});
    }
    wrapper
        .as_object_mut()
        .unwrap()
        .insert("kind".into(), Value::String(kind.into()));
    let serialized = serde_json::to_vec_pretty(&value)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    write_atomic(path, &serialized)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("json.tmp");
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all().ok();
    }
    fs::rename(&tmp, path)
}

// ---------------------------------------------------------------------------
// hooks.json rewriting
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct PluginTarget {
    hooks_json: PathBuf,
    /// Plugin variable name to embed in the command path
    /// (`CLAUDE_PLUGIN_ROOT` for Claude/Qwen, `extensionPath` for Gemini).
    plugin_var: &'static str,
    /// Hook event name (`PostToolUse` for Claude/Qwen, `AfterTool` for Gemini).
    event: &'static str,
    /// Argument passed to the wrapper to select the agent kind.
    hook_kind: &'static str,
}

fn plugin_targets() -> Vec<PluginTarget> {
    let home = paths::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
    vec![
        // Installed-plugin locations only. The dev-repo hooks.json files
        // (under plugins/ and gemini-extension/) are owned by the manifest
        // generator (STEP 9), not by `wrapper set`.
        PluginTarget {
            hooks_json: home.join(".claude/plugins/fluxmirror/hooks/hooks.json"),
            plugin_var: "CLAUDE_PLUGIN_ROOT",
            event: "PostToolUse",
            hook_kind: "claude",
        },
        PluginTarget {
            hooks_json: home.join(".qwen/plugins/fluxmirror/hooks/hooks.json"),
            plugin_var: "CLAUDE_PLUGIN_ROOT",
            event: "PostToolUse",
            hook_kind: "claude",
        },
        PluginTarget {
            hooks_json: home.join(".gemini/extensions/fluxmirror/hooks/hooks.json"),
            plugin_var: "extensionPath",
            event: "AfterTool",
            hook_kind: "gemini",
        },
    ]
}

fn shim_basename(kind: &str) -> Option<&'static str> {
    match kind {
        "bash" => Some("shim.sh"),
        "node" => Some("shim.mjs"),
        "cmd" => Some("shim.cmd"),
        "router" => Some("router.sh"),
        _ => None,
    }
}

fn shim_relpath(kind: &str) -> Option<String> {
    shim_basename(kind).map(|b| format!("wrappers/{b}"))
}

fn build_command(target: &PluginTarget, kind: &str) -> String {
    let basename = shim_basename(kind).unwrap_or("router.sh");
    let plugin_var = target.plugin_var;
    let arg = target.hook_kind;
    match kind {
        // shim.mjs needs an interpreter; everything else is directly executable.
        "node" => format!("node ${{{plugin_var}}}/wrappers/{basename} {arg}"),
        _ => format!("${{{plugin_var}}}/wrappers/{basename} {arg}"),
    }
}

fn rewrite_hooks_json(target: &PluginTarget, kind: &str) -> std::io::Result<bool> {
    let command = build_command(target, kind);
    let manifest = json!({
        "hooks": {
            target.event: [
                {
                    "hooks": [
                        {
                            "type": "command",
                            "command": command,
                        }
                    ]
                }
            ]
        }
    });
    let bytes = serde_json::to_vec_pretty(&manifest)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    if let Some(parent) = target.hooks_json.parent() {
        fs::create_dir_all(parent)?;
    }
    write_atomic(&target.hooks_json, &bytes)?;
    Ok(true)
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmd::util::test_helpers::{env_lock, EnvGuard};

    #[test]
    fn set_validates_kind() {
        let _lock = env_lock();
        let tmp = tempfile::tempdir().unwrap();
        let _h = EnvGuard::set("HOME", tmp.path().to_str().unwrap());
        let _u = EnvGuard::unset("USERPROFILE");
        let code = set("bogus");
        // ExitCode doesn't expose its raw inner; compare via Debug.
        assert_eq!(format!("{code:?}"), format!("{:?}", ExitCode::from(2)));
    }

    #[test]
    fn set_updates_config_json() {
        let _lock = env_lock();
        let tmp = tempfile::tempdir().unwrap();
        let _h = EnvGuard::set("HOME", tmp.path().to_str().unwrap());
        let _u = EnvGuard::unset("USERPROFILE");

        let _code = set("bash");

        let cfg = config_json_path();
        let v: Value = serde_json::from_slice(&fs::read(&cfg).unwrap()).unwrap();
        assert_eq!(v["wrapper"]["kind"], "bash");
    }

    #[test]
    fn set_preserves_other_config_keys() {
        let _lock = env_lock();
        let tmp = tempfile::tempdir().unwrap();
        let _h = EnvGuard::set("HOME", tmp.path().to_str().unwrap());
        let _u = EnvGuard::unset("USERPROFILE");

        let cfg = config_json_path();
        fs::create_dir_all(cfg.parent().unwrap()).unwrap();
        fs::write(&cfg, br#"{"language":"english"}"#).unwrap();

        let _code = set("node");
        let v: Value = serde_json::from_slice(&fs::read(&cfg).unwrap()).unwrap();
        assert_eq!(v["language"], "english");
        assert_eq!(v["wrapper"]["kind"], "node");
    }

    #[test]
    fn shim_basename_table() {
        assert_eq!(shim_basename("bash"), Some("shim.sh"));
        assert_eq!(shim_basename("node"), Some("shim.mjs"));
        assert_eq!(shim_basename("cmd"), Some("shim.cmd"));
        assert_eq!(shim_basename("router"), Some("router.sh"));
        assert_eq!(shim_basename("garbage"), None);
    }

    #[test]
    fn build_command_uses_plugin_var_and_kind() {
        let claude = PluginTarget {
            hooks_json: PathBuf::from("/tmp/x/hooks.json"),
            plugin_var: "CLAUDE_PLUGIN_ROOT",
            event: "PostToolUse",
            hook_kind: "claude",
        };
        let gemini = PluginTarget {
            hooks_json: PathBuf::from("/tmp/y/hooks.json"),
            plugin_var: "extensionPath",
            event: "AfterTool",
            hook_kind: "gemini",
        };
        assert_eq!(
            build_command(&claude, "bash"),
            "${CLAUDE_PLUGIN_ROOT}/wrappers/shim.sh claude"
        );
        assert_eq!(
            build_command(&gemini, "node"),
            "node ${extensionPath}/wrappers/shim.mjs gemini"
        );
        assert_eq!(
            build_command(&gemini, "router"),
            "${extensionPath}/wrappers/router.sh gemini"
        );
    }
}
