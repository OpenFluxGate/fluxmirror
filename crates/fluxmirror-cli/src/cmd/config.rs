// fluxmirror config — read / write / inspect config layers.
//
// Sub-operations:
//   show     — pretty-print the merged Config as JSON
//   get K    — dot-path read on the merged Config
//   set K V  — dot-path write into the user file (atomic)
//   explain  — print each key + the layer that defined it
//
// Layering (low → high precedence):
//   defaults → inferred → user-file → project-file → env
//
// CLI flags would be the highest layer but they don't apply to `config`
// itself, so they're not modelled here.

use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Subcommand;
use fluxmirror_core::{paths, tz, Config, Language};
use serde_json::{json, Value};

#[derive(Subcommand)]
pub enum ConfigOp {
    /// Print the resolved value of a single key.
    Get { key: String },
    /// Set a key in the user config layer.
    Set { key: String, value: String },
    /// Print the fully-resolved config (all layers merged).
    Show,
    /// Print each key with the layer that won.
    Explain,
}

pub fn run(op: ConfigOp) -> ExitCode {
    match op {
        ConfigOp::Show => show(),
        ConfigOp::Get { key } => get(&key),
        ConfigOp::Set { key, value } => set(&key, &value),
        ConfigOp::Explain => explain(),
    }
}

// ---------------------------------------------------------------------------
// show
// ---------------------------------------------------------------------------

fn show() -> ExitCode {
    match Config::load() {
        Ok(cfg) => match serde_json::to_string_pretty(&cfg) {
            Ok(s) => {
                println!("{s}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("fluxmirror config show: serialize failed: {e}");
                ExitCode::from(1)
            }
        },
        Err(e) => {
            eprintln!("fluxmirror config show: load failed: {e}");
            ExitCode::from(1)
        }
    }
}

// ---------------------------------------------------------------------------
// get
// ---------------------------------------------------------------------------

fn get(key: &str) -> ExitCode {
    let cfg = match Config::load() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("fluxmirror config get: {e}");
            return ExitCode::from(1);
        }
    };
    let v = match serde_json::to_value(&cfg) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("fluxmirror config get: serialize: {e}");
            return ExitCode::from(1);
        }
    };
    match dot_get(&v, key) {
        Some(found) => {
            // Strings are unquoted; everything else round-trips through JSON.
            if let Some(s) = found.as_str() {
                println!("{s}");
            } else {
                println!("{found}");
            }
            ExitCode::SUCCESS
        }
        None => {
            // Empty stdout + exit 1 so shell pipelines distinguish "missing".
            ExitCode::from(1)
        }
    }
}

// ---------------------------------------------------------------------------
// set
// ---------------------------------------------------------------------------

fn set(key: &str, value: &str) -> ExitCode {
    if let Err(msg) = validate(key, value) {
        eprintln!("fluxmirror config set: {msg}");
        return ExitCode::from(2);
    }

    let cfg_path = paths::config_dir().join("config.json");
    let mut value_json: Value = if cfg_path.exists() {
        match fs::read(&cfg_path) {
            Ok(b) if !b.is_empty() => {
                serde_json::from_slice(&b).unwrap_or_else(|_| json!({}))
            }
            _ => json!({}),
        }
    } else {
        // Seed with the defaults serialised as JSON so first-time
        // `config set` gives a complete file rather than `{"x":...}`
        // alone.
        match serde_json::to_value(Config::default()) {
            Ok(v) => v,
            Err(_) => json!({}),
        }
    };
    if !value_json.is_object() {
        value_json = json!({});
    }

    let parsed = parse_value_for_key(key, value);
    if dot_set(&mut value_json, key, parsed).is_err() {
        eprintln!("fluxmirror config set: invalid key path {:?}", key);
        return ExitCode::from(2);
    }

    if let Err(e) = save_atomic(&cfg_path, &value_json) {
        eprintln!(
            "fluxmirror config set: failed to write {}: {e}",
            cfg_path.display()
        );
        return ExitCode::from(1);
    }
    println!("set {key} = {value}");
    ExitCode::SUCCESS
}

// ---------------------------------------------------------------------------
// explain
// ---------------------------------------------------------------------------

fn explain() -> ExitCode {
    // We re-walk the layers ourselves so we can attribute each key.
    let user_path = paths::config_dir().join("config.json");
    let project_path = PathBuf::from(".fluxmirror.toml");
    let user_value = read_json(&user_path);
    let project_present = project_path.exists();

    // For each key, walk env → user-file → project-file → inferred → default.
    let inferred_lang = inferred_language();
    let inferred_tz = tz::infer_default_tz().name().to_string();

    println!("{:<22} {:<24} {}", "key", "value", "source");

    explain_one(
        "language",
        &[
            ("env (FLUXMIRROR_LANGUAGE)", env_lang()),
            ("user-file", user_str(&user_value, "language")),
            ("project-file", project_placeholder(project_present, "language")),
            ("inferred", Some(inferred_lang.as_str().to_string())),
            ("default", Some(Language::English.as_str().to_string())),
        ],
    );
    explain_one(
        "timezone",
        &[
            ("env (FLUXMIRROR_TIMEZONE)", env::var("FLUXMIRROR_TIMEZONE").ok()),
            ("user-file", user_str(&user_value, "timezone")),
            ("project-file", project_placeholder(project_present, "timezone")),
            ("inferred", Some(inferred_tz)),
            ("default", Some("UTC".to_string())),
        ],
    );
    explain_one(
        "wrapper.kind",
        &[
            ("user-file", user_str_path(&user_value, &["wrapper", "kind"])),
            ("project-file", project_placeholder(project_present, "wrapper.kind")),
            ("default", Some("auto".to_string())),
        ],
    );
    explain_one(
        "storage.path",
        &[
            ("env (FLUXMIRROR_DB)", env::var("FLUXMIRROR_DB").ok()),
            ("user-file", user_str_path(&user_value, &["storage", "path"])),
            ("project-file", project_placeholder(project_present, "storage.path")),
            ("default", Some("<unset>".to_string())),
        ],
    );
    explain_one(
        "storage.retention_days",
        &[
            ("user-file", user_str_path(&user_value, &["storage", "retention_days"])),
            ("project-file", project_placeholder(project_present, "storage.retention_days")),
            ("default", Some("<unset>".to_string())),
        ],
    );

    ExitCode::SUCCESS
}

fn explain_one(key: &str, layers: &[(&str, Option<String>)]) {
    for (source, val) in layers {
        if let Some(v) = val {
            if v.is_empty() {
                continue;
            }
            println!("{:<22} {:<24} {}", key, v, source);
            return;
        }
    }
    println!("{:<22} {:<24} {}", key, "<unset>", "default");
}

fn env_lang() -> Option<String> {
    env::var("FLUXMIRROR_LANGUAGE").ok().map(|raw| {
        // Normalise locale-style strings to our canonical lowercase form.
        Language::from_locale(&raw).as_str().to_string()
    })
}

fn inferred_language() -> Language {
    for key in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(v) = env::var(key) {
            if !v.is_empty() {
                return Language::from_locale(&v);
            }
        }
    }
    Language::English
}

fn read_json(path: &Path) -> Option<Value> {
    let bytes = fs::read(path).ok()?;
    if bytes.is_empty() {
        return None;
    }
    serde_json::from_slice(&bytes).ok()
}

fn user_str(value: &Option<Value>, key: &str) -> Option<String> {
    let v = value.as_ref()?.get(key)?;
    Some(value_to_display(v))
}

fn user_str_path(value: &Option<Value>, path: &[&str]) -> Option<String> {
    let mut cur = value.as_ref()?;
    for seg in path {
        cur = cur.get(*seg)?;
    }
    Some(value_to_display(cur))
}

fn value_to_display(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn project_placeholder(present: bool, _key: &str) -> Option<String> {
    // Project-file parsing is a STEP 8 stub in core::config; until the
    // `toml` crate lands, we surface its presence without claiming any
    // override actually applied.
    if present {
        // Returning None lets the next layer win — accurate today.
        None
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// dot-path helpers (read + write)
// ---------------------------------------------------------------------------

fn dot_get<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    let mut cur = value;
    for seg in key.split('.') {
        cur = cur.get(seg)?;
    }
    Some(cur)
}

fn dot_set(value: &mut Value, key: &str, new_value: Value) -> Result<(), ()> {
    let parts: Vec<&str> = key.split('.').collect();
    if parts.is_empty() || parts.iter().any(|s| s.is_empty()) {
        return Err(());
    }
    let mut cur = value;
    for seg in &parts[..parts.len() - 1] {
        if !cur.is_object() {
            return Err(());
        }
        let obj = cur.as_object_mut().ok_or(())?;
        cur = obj
            .entry((*seg).to_string())
            .or_insert_with(|| json!({}));
        if !cur.is_object() {
            *cur = json!({});
        }
    }
    let obj = cur.as_object_mut().ok_or(())?;
    obj.insert(parts[parts.len() - 1].to_string(), new_value);
    Ok(())
}

/// Coerce string CLI input into the right JSON type for known keys so
/// the resulting config.json round-trips through serde cleanly.
fn parse_value_for_key(key: &str, raw: &str) -> Value {
    match key {
        "schema_version" | "storage.retention_days" => {
            if let Ok(n) = raw.parse::<u64>() {
                return Value::Number(n.into());
            }
            Value::String(raw.to_string())
        }
        "self_noise.enabled"
        | "agents.claude-code.enabled"
        | "agents.qwen-code.enabled"
        | "agents.gemini-cli.enabled"
        | "agents.claude-desktop.enabled"
        | "wrapper.auto_detected" => match raw.to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" | "y" => Value::Bool(true),
            "false" | "0" | "no" | "n" => Value::Bool(false),
            _ => Value::String(raw.to_string()),
        },
        _ => Value::String(raw.to_string()),
    }
}

fn validate(key: &str, value: &str) -> Result<(), String> {
    match key {
        "language" => {
            // Accept either canonical names or our locale shortcuts.
            let v = value.to_ascii_lowercase();
            let ok = matches!(
                v.as_str(),
                "english"
                    | "korean"
                    | "japanese"
                    | "chinese"
                    | "en"
                    | "ko"
                    | "kr"
                    | "ja"
                    | "zh"
            );
            if !ok {
                return Err(format!(
                    "invalid language {:?} (expected: english | korean | japanese | chinese)",
                    value
                ));
            }
        }
        "timezone" => {
            value
                .parse::<chrono_tz::Tz>()
                .map(|_| ())
                .map_err(|_| format!("invalid timezone {:?}", value))?;
        }
        "wrapper.kind" => {
            if !matches!(
                value,
                "bash" | "node" | "cmd" | "pwsh" | "router" | "auto"
            ) {
                return Err(format!(
                    "invalid wrapper.kind {:?} (expected: bash | node | cmd | pwsh | router | auto)",
                    value
                ));
            }
        }
        _ => {}
    }
    Ok(())
}

fn save_atomic(path: &Path, value: &Value) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    let tmp = path.with_extension("json.tmp");
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(&bytes)?;
        f.sync_all().ok();
    }
    fs::rename(&tmp, path)
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmd::util::test_helpers::{env_lock, EnvGuard};

    fn read_cfg() -> Value {
        let p = paths::config_dir().join("config.json");
        let bytes = fs::read(&p).expect("config.json should exist");
        serde_json::from_slice(&bytes).unwrap()
    }

    #[test]
    fn dot_get_walks_nested_keys() {
        let v: Value = serde_json::from_str(
            r#"{"a":{"b":{"c":"hi"}},"x":42}"#,
        )
        .unwrap();
        assert_eq!(dot_get(&v, "a.b.c").unwrap(), &Value::String("hi".into()));
        assert_eq!(dot_get(&v, "x").unwrap(), &Value::Number(42.into()));
        assert!(dot_get(&v, "a.missing").is_none());
    }

    #[test]
    fn dot_set_creates_nested_objects() {
        let mut v = json!({});
        dot_set(&mut v, "a.b.c", Value::String("hi".into())).unwrap();
        assert_eq!(v["a"]["b"]["c"], "hi");
    }

    #[test]
    fn set_then_get_round_trip() {
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

        let code = set("language", "korean");
        assert_eq!(format!("{code:?}"), format!("{:?}", ExitCode::SUCCESS));
        // Verify file landed and load() respects it.
        let v = read_cfg();
        assert_eq!(v["language"], "korean");

        // get() prints to stdout, so we just assert exit code.
        let code2 = get("language");
        assert_eq!(format!("{code2:?}"), format!("{:?}", ExitCode::SUCCESS));
    }

    #[test]
    fn show_emits_valid_json() {
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

        // We can't easily capture stdout from inside the test without
        // extra plumbing, but we can verify the loader path that show()
        // walks produces valid JSON.
        let cfg = Config::load().unwrap();
        let s = serde_json::to_string_pretty(&cfg).unwrap();
        let v: Value = serde_json::from_str(&s).unwrap();
        assert!(v.is_object());
        assert!(v.get("language").is_some());
        assert!(v.get("timezone").is_some());
    }

    #[test]
    fn explain_reports_user_file_source() {
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

        // Seed a user-file with a non-default language.
        let _ = set("language", "korean");
        // Walk the explain layers manually via the same helpers and
        // verify user-file wins.
        let user_path = paths::config_dir().join("config.json");
        let user_value = read_json(&user_path);
        assert_eq!(user_str(&user_value, "language").as_deref(), Some("korean"));
    }

    #[test]
    fn validate_rejects_bad_language() {
        assert!(validate("language", "klingon").is_err());
    }

    #[test]
    fn validate_rejects_bad_timezone() {
        assert!(validate("timezone", "Atlantis/Lost").is_err());
    }

    #[test]
    fn validate_accepts_known_wrapper_kinds() {
        for k in ["bash", "node", "cmd", "pwsh", "router", "auto"] {
            assert!(validate("wrapper.kind", k).is_ok(), "kind {k} rejected");
        }
        assert!(validate("wrapper.kind", "garbage").is_err());
    }
}
