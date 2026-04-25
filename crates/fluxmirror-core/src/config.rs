// Layered configuration.
//
// Effective layering (low → high precedence):
//
//   1. compiled-in defaults     (Config::default)
//   2. inferred                 (locale → language, system → timezone)
//   3. user file                (~/.fluxmirror/config.json)
//   4. project file             (./.fluxmirror.toml)   [STEP 8 finishes parser]
//   5. environment variables    (FLUXMIRROR_*)
//   6. CLI flags                (applied by callers, after `load()`)
//
// `Config::load` walks layers 1-5 and returns the merged result. CLI
// overrides are intentionally NOT touched here so this module stays
// independent of the clap layer.

use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    English,
    Korean,
    Japanese,
    Chinese,
}

impl Language {
    pub fn as_str(&self) -> &'static str {
        match self {
            Language::English => "english",
            Language::Korean => "korean",
            Language::Japanese => "japanese",
            Language::Chinese => "chinese",
        }
    }

    /// Map a POSIX locale string ("ko_KR.UTF-8", "ja", "en_US") to a
    /// supported `Language`. Anything unrecognised falls back to English.
    pub fn from_locale(locale: &str) -> Self {
        let prefix = locale
            .split('_')
            .next()
            .unwrap_or(locale)
            .split('.')
            .next()
            .unwrap_or(locale)
            .to_ascii_lowercase();
        match prefix.as_str() {
            "ko" | "kr" => Language::Korean,
            "ja" => Language::Japanese,
            "zh" => Language::Chinese,
            "korean" => Language::Korean,
            "japanese" => Language::Japanese,
            "chinese" => Language::Chinese,
            _ => Language::English,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WrapperKind {
    Bash,
    Node,
    Cmd,
    Pwsh,
    Auto,
}

impl Default for WrapperKind {
    fn default() -> Self {
        WrapperKind::Auto
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct StorageConfig {
    pub kind: String,
    pub path: Option<PathBuf>,
    pub retention_days: Option<u32>,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            kind: "sqlite".into(),
            path: None,
            retention_days: None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct SelfNoiseConfig {
    pub enabled: bool,
    pub repo_paths: Vec<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct AgentToggle {
    pub enabled: bool,
}

impl Default for AgentToggle {
    fn default() -> Self {
        Self { enabled: true }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct AgentsConfig {
    #[serde(rename = "claude-code")]
    pub claude_code: AgentToggle,
    #[serde(rename = "qwen-code")]
    pub qwen_code: AgentToggle,
    #[serde(rename = "gemini-cli")]
    pub gemini_cli: AgentToggle,
    #[serde(rename = "claude-desktop")]
    pub claude_desktop: AgentToggle,
}

impl Default for AgentsConfig {
    fn default() -> Self {
        Self {
            claude_code: AgentToggle::default(),
            qwen_code: AgentToggle::default(),
            gemini_cli: AgentToggle::default(),
            claude_desktop: AgentToggle::default(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct WrapperConfig {
    pub kind: WrapperKind,
    pub path: Option<PathBuf>,
    pub selected_at: Option<String>,
    pub auto_detected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Config {
    pub schema_version: u32,
    pub language: Language,
    pub timezone: String,
    pub storage: StorageConfig,
    pub self_noise: SelfNoiseConfig,
    pub agents: AgentsConfig,
    pub wrapper: WrapperConfig,
    // Phase 2/3 slots intentionally absent until they ship:
    //   pub daemon, forward, redaction, telemetry
}

impl Default for Config {
    fn default() -> Self {
        Self {
            schema_version: 1,
            language: Language::English,
            timezone: "UTC".into(),
            storage: StorageConfig::default(),
            self_noise: SelfNoiseConfig::default(),
            agents: AgentsConfig::default(),
            wrapper: WrapperConfig::default(),
        }
    }
}

/// Provenance tag used by `fluxmirror config explain` to show which
/// layer set each effective value. Layers are listed low → high
/// precedence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigSource {
    Default,
    Inferred,
    UserFile,
    ProjectFile,
    Env,
    Cli,
}

impl Config {
    pub fn user_path() -> PathBuf {
        crate::paths::config_dir().join("config.json")
    }

    pub fn project_path() -> PathBuf {
        PathBuf::from(".fluxmirror.toml")
    }

    /// Walk layers 1-5 (defaults → inferred → user file → project file →
    /// env). CLI overrides are applied by the caller after this returns.
    pub fn load() -> crate::Result<Self> {
        // Layer 1: compiled-in defaults
        let mut cfg = Self::default();

        // Layer 2: inferred (locale, system tz)
        if let Ok(lang) = env::var("LANG") {
            cfg.language = Language::from_locale(&lang);
        }
        cfg.timezone = crate::tz::infer_default_tz().name().to_string();

        // Layer 3: user file
        let up = Self::user_path();
        if up.exists() {
            let s = fs::read_to_string(&up)?;
            if !s.trim().is_empty() {
                cfg = serde_json::from_str(&s)?;
            }
        }

        // Layer 4: project file (TOML)
        let pp = Self::project_path();
        if pp.exists() {
            let s = fs::read_to_string(&pp)?;
            // TODO STEP 8: real TOML parse via the `toml` crate. STEP 2
            // intentionally avoids the extra dep so the boundary is
            // already defined when STEP 8 fills it in.
            let _ = s;
        }

        // Layer 5: environment variables
        if let Ok(lang) = env::var("FLUXMIRROR_LANGUAGE") {
            cfg.language = Language::from_locale(&lang);
        }
        if let Ok(tz) = env::var("FLUXMIRROR_TIMEZONE") {
            cfg.timezone = tz;
        }
        if let Ok(p) = env::var("FLUXMIRROR_DB") {
            cfg.storage.path = Some(PathBuf::from(p));
        }

        Ok(cfg)
    }

    /// Atomically persist to the user file. Creates parents on demand.
    pub fn save(&self) -> crate::Result<()> {
        let p = Self::user_path();
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent)?;
        }
        let s = serde_json::to_string_pretty(self)?;
        let tmp = p.with_extension("json.tmp");
        fs::write(&tmp, s)?;
        fs::rename(&tmp, &p)?;
        Ok(())
    }

    /// Effective DB path: explicit storage.path overrides the OS default.
    pub fn effective_db_path(&self) -> PathBuf {
        self.storage
            .path
            .clone()
            .unwrap_or_else(crate::paths::default_db_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Env-mutating tests must not race; serialize them with a mutex.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct EnvGuard {
        key: &'static str,
        prior: Option<std::ffi::OsString>,
    }
    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let prior = env::var_os(key);
            env::set_var(key, value);
            Self { key, prior }
        }
        fn unset(key: &'static str) -> Self {
            let prior = env::var_os(key);
            env::remove_var(key);
            Self { key, prior }
        }
    }
    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match self.prior.take() {
                Some(v) => env::set_var(self.key, v),
                None => env::remove_var(self.key),
            }
        }
    }

    #[test]
    fn default_round_trips_via_json() {
        let c = Config::default();
        let s = serde_json::to_string(&c).unwrap();
        let back: Config = serde_json::from_str(&s).unwrap();
        assert_eq!(c, back);
    }

    #[test]
    fn language_from_locale_maps_known_prefixes() {
        assert_eq!(Language::from_locale("ko_KR.UTF-8"), Language::Korean);
        assert_eq!(Language::from_locale("ja_JP"), Language::Japanese);
        assert_eq!(Language::from_locale("zh"), Language::Chinese);
        assert_eq!(Language::from_locale("en_US"), Language::English);
        assert_eq!(Language::from_locale("xx_YY"), Language::English);
        // explicit "korean" string also accepted (env var convenience)
        assert_eq!(Language::from_locale("korean"), Language::Korean);
    }

    #[test]
    fn load_env_overrides_inferred_language_and_tz() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // Point HOME at a tempdir so the user-file branch is a no-op.
        let tmp = tempfile::tempdir().unwrap();
        let _h = EnvGuard::set("HOME", tmp.path().to_str().unwrap());
        let _u = EnvGuard::unset("USERPROFILE");
        let _l = EnvGuard::set("LANG", "en_US.UTF-8");
        let _fl = EnvGuard::set("FLUXMIRROR_LANGUAGE", "korean");
        let _ft = EnvGuard::set("FLUXMIRROR_TIMEZONE", "Asia/Seoul");
        let _fd = EnvGuard::unset("FLUXMIRROR_DB");

        let c = Config::load().unwrap();
        assert_eq!(c.language, Language::Korean);
        assert_eq!(c.timezone, "Asia/Seoul");
        assert_eq!(c.schema_version, 1);
    }

    #[test]
    fn load_storage_path_env_override() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        let _h = EnvGuard::set("HOME", tmp.path().to_str().unwrap());
        let _u = EnvGuard::unset("USERPROFILE");
        let _fl = EnvGuard::unset("FLUXMIRROR_LANGUAGE");
        let _ft = EnvGuard::unset("FLUXMIRROR_TIMEZONE");
        let _fd = EnvGuard::set("FLUXMIRROR_DB", "/tmp/mydb.db");

        let c = Config::load().unwrap();
        assert_eq!(c.storage.path, Some(PathBuf::from("/tmp/mydb.db")));
        assert_eq!(c.effective_db_path(), PathBuf::from("/tmp/mydb.db"));
    }

    #[test]
    fn save_then_load_round_trip() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        let _h = EnvGuard::set("HOME", tmp.path().to_str().unwrap());
        let _u = EnvGuard::unset("USERPROFILE");
        let _fl = EnvGuard::unset("FLUXMIRROR_LANGUAGE");
        let _ft = EnvGuard::unset("FLUXMIRROR_TIMEZONE");
        let _fd = EnvGuard::unset("FLUXMIRROR_DB");

        let mut c = Config::default();
        c.language = Language::Japanese;
        c.timezone = "Asia/Tokyo".into();
        c.save().unwrap();

        let loaded = Config::load().unwrap();
        assert_eq!(loaded.language, Language::Japanese);
        assert_eq!(loaded.timezone, "Asia/Tokyo");
    }

    #[test]
    fn agent_toggle_default_enabled() {
        assert!(AgentToggle::default().enabled);
        let cfg = AgentsConfig::default();
        assert!(cfg.claude_code.enabled);
        assert!(cfg.qwen_code.enabled);
        assert!(cfg.gemini_cli.enabled);
        assert!(cfg.claude_desktop.enabled);
    }

    #[test]
    fn wrapper_config_defaults_to_auto() {
        let w = WrapperConfig::default();
        assert_eq!(w.kind, WrapperKind::Auto);
        assert!(w.path.is_none());
        assert!(!w.auto_detected);
    }
}
