// Layered configuration.
//
// Effective layering (low → high precedence):
//
//   1. compiled-in defaults     (Config::default)
//   2. inferred                 (locale → language, system → timezone)
//   3. user file                (~/.fluxmirror/config.json)
//   4. project file             (./.fluxmirror.toml — Phase 3 M9)
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
pub struct StudioConfig {
    pub port: u16,
    pub host: String,
    pub enable_llm_naming: bool,
}

impl Default for StudioConfig {
    fn default() -> Self {
        Self {
            port: 7090,
            host: "127.0.0.1".into(),
            enable_llm_naming: false,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct RedactionConfig {
    /// Extra regex patterns layered on top of the built-in set. The
    /// strings are validated as `regex::Regex` at load time by the
    /// redaction layer; this struct only stores them.
    pub patterns: Vec<String>,
}

/// AI service configuration. Consumed by the `fluxmirror-ai` crate.
/// `provider = "off"` short-circuits every synthesise() call so callers
/// can take a heuristic path with zero overhead.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AiConfig {
    /// `"anthropic"` | `"ollama"` | `"off"`.
    pub provider: String,
    /// Default model for daily / session / anomaly prompts.
    pub default_model: String,
    /// Heavier model for project-arc prompts.
    pub project_model: String,
    /// USD ceiling per local-day. Atomic file at `~/.fluxmirror/ai-budget-<YYYY-MM-DD>.txt`.
    pub daily_budget_usd: f64,
    /// `ai_cache` row TTL.
    pub cache_ttl_days: u32,
    /// Per-prompt user-message cap, in chars (not bytes). Truncates with
    /// a sentinel when exceeded.
    pub max_user_chars: usize,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            provider: "anthropic".into(),
            default_model: "claude-haiku-4-5-20251001".into(),
            project_model: "claude-sonnet-4-6".into(),
            daily_budget_usd: 1.0,
            cache_ttl_days: 7,
            max_user_chars: 8192,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Config {
    pub schema_version: u32,
    pub language: Language,
    pub timezone: String,
    pub storage: StorageConfig,
    pub self_noise: SelfNoiseConfig,
    pub agents: AgentsConfig,
    pub wrapper: WrapperConfig,
    pub redaction: RedactionConfig,
    pub studio: StudioConfig,
    pub ai: AiConfig,
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
            redaction: RedactionConfig::default(),
            studio: StudioConfig::default(),
            ai: AiConfig::default(),
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

        // Layer 4: project file (TOML). Missing keys silently fall through
        // to the user-file / inferred / default value already in `cfg`.
        let pp = Self::project_path();
        if pp.exists() {
            let s = fs::read_to_string(&pp)?;
            if !s.trim().is_empty() {
                let project: ProjectToml = toml::from_str(&s)?;
                project.merge_into(&mut cfg);
            }
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

/// On-disk schema for `./.fluxmirror.toml`. Every field is optional so a
/// project file may set just one or two values without nulling the rest.
/// Unknown keys are accepted (forward-compat — older binaries should not
/// reject newer config files outright).
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ProjectToml {
    language: Option<String>,
    timezone: Option<String>,
    db_path: Option<PathBuf>,
    redaction: Option<RedactionToml>,
    studio: Option<StudioToml>,
    ai: Option<AiToml>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct RedactionToml {
    patterns: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct StudioToml {
    port: Option<u16>,
    host: Option<String>,
    enable_llm_naming: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct AiToml {
    provider: Option<String>,
    default_model: Option<String>,
    project_model: Option<String>,
    daily_budget_usd: Option<f64>,
    cache_ttl_days: Option<u32>,
    max_user_chars: Option<usize>,
}

impl ProjectToml {
    fn merge_into(self, cfg: &mut Config) {
        if let Some(lang) = self.language {
            cfg.language = Language::from_locale(&lang);
        }
        if let Some(tz) = self.timezone {
            cfg.timezone = tz;
        }
        if let Some(path) = self.db_path {
            cfg.storage.path = Some(path);
        }
        if let Some(red) = self.redaction {
            if !red.patterns.is_empty() {
                cfg.redaction.patterns = red.patterns;
            }
        }
        if let Some(studio) = self.studio {
            if let Some(port) = studio.port {
                cfg.studio.port = port;
            }
            if let Some(host) = studio.host {
                cfg.studio.host = host;
            }
            if let Some(flag) = studio.enable_llm_naming {
                cfg.studio.enable_llm_naming = flag;
            }
        }
        if let Some(ai) = self.ai {
            if let Some(provider) = ai.provider {
                cfg.ai.provider = provider;
            }
            if let Some(model) = ai.default_model {
                cfg.ai.default_model = model;
            }
            if let Some(model) = ai.project_model {
                cfg.ai.project_model = model;
            }
            if let Some(usd) = ai.daily_budget_usd {
                cfg.ai.daily_budget_usd = usd;
            }
            if let Some(ttl) = ai.cache_ttl_days {
                cfg.ai.cache_ttl_days = ttl;
            }
            if let Some(cap) = ai.max_user_chars {
                cfg.ai.max_user_chars = cap;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let _lock = crate::test_lock::env_lock();
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
        let _lock = crate::test_lock::env_lock();
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
        let _lock = crate::test_lock::env_lock();
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

    #[test]
    fn studio_config_defaults() {
        let s = StudioConfig::default();
        assert_eq!(s.port, 7090);
        assert_eq!(s.host, "127.0.0.1");
        assert!(!s.enable_llm_naming);
    }

    #[test]
    fn redaction_config_defaults_empty() {
        assert!(RedactionConfig::default().patterns.is_empty());
    }

    fn parse_project(s: &str) -> ProjectToml {
        toml::from_str(s).expect("parse project toml")
    }

    #[test]
    fn project_toml_empty_string_does_not_override() {
        let mut cfg = Config::default();
        let project = parse_project("");
        project.merge_into(&mut cfg);
        assert_eq!(cfg, Config::default());
    }

    #[test]
    fn project_toml_overrides_top_level_keys() {
        let mut cfg = Config::default();
        let project = parse_project(
            r#"
language = "korean"
timezone = "Asia/Seoul"
db_path = "/custom/events.db"
"#,
        );
        project.merge_into(&mut cfg);
        assert_eq!(cfg.language, Language::Korean);
        assert_eq!(cfg.timezone, "Asia/Seoul");
        assert_eq!(cfg.storage.path, Some(PathBuf::from("/custom/events.db")));
    }

    #[test]
    fn project_toml_overrides_studio_subtable() {
        let mut cfg = Config::default();
        let project = parse_project(
            r#"
[studio]
port = 8088
host = "0.0.0.0"
enable_llm_naming = true
"#,
        );
        project.merge_into(&mut cfg);
        assert_eq!(cfg.studio.port, 8088);
        assert_eq!(cfg.studio.host, "0.0.0.0");
        assert!(cfg.studio.enable_llm_naming);
    }

    #[test]
    fn project_toml_partial_studio_keeps_defaults() {
        let mut cfg = Config::default();
        let project = parse_project(r#"[studio]
port = 9090
"#);
        project.merge_into(&mut cfg);
        assert_eq!(cfg.studio.port, 9090);
        // host left at default
        assert_eq!(cfg.studio.host, "127.0.0.1");
        assert!(!cfg.studio.enable_llm_naming);
    }

    #[test]
    fn project_toml_redaction_patterns_appended() {
        let mut cfg = Config::default();
        let project = parse_project(
            r#"
[redaction]
patterns = ["my-token-[a-z0-9]{16}", "internal-id-\\d+"]
"#,
        );
        project.merge_into(&mut cfg);
        assert_eq!(cfg.redaction.patterns.len(), 2);
        assert_eq!(cfg.redaction.patterns[0], "my-token-[a-z0-9]{16}");
    }

    #[test]
    fn project_toml_bad_input_returns_error_not_panic() {
        let r: std::result::Result<ProjectToml, toml::de::Error> =
            toml::from_str("language = 7\n[studio\nport = 'not-a-number'");
        assert!(r.is_err());
    }

    #[test]
    fn load_project_file_overrides_user_file_and_inferred() {
        let _lock = crate::test_lock::env_lock();
        let tmp = tempfile::tempdir().unwrap();
        let _h = EnvGuard::set("HOME", tmp.path().to_str().unwrap());
        let _u = EnvGuard::unset("USERPROFILE");
        let _l = EnvGuard::set("LANG", "en_US.UTF-8");
        let _fl = EnvGuard::unset("FLUXMIRROR_LANGUAGE");
        let _ft = EnvGuard::unset("FLUXMIRROR_TIMEZONE");
        let _fd = EnvGuard::unset("FLUXMIRROR_DB");

        let prev = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();
        std::fs::write(
            tmp.path().join(".fluxmirror.toml"),
            "language = \"japanese\"\ntimezone = \"Asia/Tokyo\"\n",
        )
        .unwrap();

        let c = Config::load().unwrap();
        std::env::set_current_dir(prev).unwrap();

        assert_eq!(c.language, Language::Japanese);
        assert_eq!(c.timezone, "Asia/Tokyo");
    }

    #[test]
    fn env_var_beats_project_file() {
        let _lock = crate::test_lock::env_lock();
        let tmp = tempfile::tempdir().unwrap();
        let _h = EnvGuard::set("HOME", tmp.path().to_str().unwrap());
        let _u = EnvGuard::unset("USERPROFILE");
        let _l = EnvGuard::unset("LANG");
        let _fl = EnvGuard::set("FLUXMIRROR_LANGUAGE", "chinese");
        let _ft = EnvGuard::unset("FLUXMIRROR_TIMEZONE");
        let _fd = EnvGuard::unset("FLUXMIRROR_DB");

        let prev = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();
        std::fs::write(
            tmp.path().join(".fluxmirror.toml"),
            "language = \"japanese\"\n",
        )
        .unwrap();

        let c = Config::load().unwrap();
        std::env::set_current_dir(prev).unwrap();

        // env layer (Chinese) wins over project file (Japanese).
        assert_eq!(c.language, Language::Chinese);
    }
}
