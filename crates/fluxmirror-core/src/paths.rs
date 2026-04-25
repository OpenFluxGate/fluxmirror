// OS-aware filesystem locations.
//
// Centralizing these here keeps `cfg(target_os = "...")` proliferation
// out of the rest of the crate. Most Phase 1 callers just want
// `default_db_path()` or `config_dir()` and don't care which OS they're
// on.

use std::env;
use std::path::PathBuf;

/// Resolve user home: HOME first (POSIX), USERPROFILE second (Windows).
/// `None` if neither is set.
pub fn home_dir() -> Option<PathBuf> {
    if let Some(h) = env::var_os("HOME").filter(|s| !s.is_empty()) {
        return Some(PathBuf::from(h));
    }
    if let Some(h) = env::var_os("USERPROFILE").filter(|s| !s.is_empty()) {
        return Some(PathBuf::from(h));
    }
    None
}

/// Default location for the SQLite events DB.
///
/// Precedence:
///   1. `FLUXMIRROR_DB` env override (any OS)
///   2. macOS:   `~/Library/Application Support/fluxmirror/events.db`
///   3. Windows: `%APPDATA%/fluxmirror/events.db`
///              (falls back to `~/AppData/Roaming/fluxmirror/events.db`)
///   4. Linux/other: `${XDG_DATA_HOME}/fluxmirror/events.db`
///                   (falls back to `~/.local/share/fluxmirror/events.db`)
pub fn default_db_path() -> PathBuf {
    if let Some(p) = env::var_os("FLUXMIRROR_DB").filter(|s| !s.is_empty()) {
        return PathBuf::from(p);
    }
    let home = home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
    #[cfg(target_os = "macos")]
    {
        home.join("Library/Application Support/fluxmirror/events.db")
    }
    #[cfg(target_os = "windows")]
    {
        if let Some(appdata) = env::var_os("APPDATA").filter(|s| !s.is_empty()) {
            return PathBuf::from(appdata).join("fluxmirror").join("events.db");
        }
        home.join("AppData/Roaming/fluxmirror/events.db")
    }
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    {
        if let Some(xdg) = env::var_os("XDG_DATA_HOME").filter(|s| !s.is_empty()) {
            return PathBuf::from(xdg).join("fluxmirror").join("events.db");
        }
        home.join(".local/share/fluxmirror/events.db")
    }
}

/// The legacy macOS DB path that pre-Phase-1 hooks wrote to on every
/// platform (Linux included, due to a hardcoded join). Exposed so that
/// `fluxmirror doctor` (STEP 8) can detect and warn about migrations.
pub fn legacy_macos_db_path() -> PathBuf {
    let home = home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
    home.join("Library/Application Support/fluxmirror/events.db")
}

/// User config directory — `~/.fluxmirror/` on every OS for now.
/// STEP 6 may diversify (XDG_CONFIG_HOME on Linux, %APPDATA% on Windows)
/// once the migration story is decided.
pub fn config_dir() -> PathBuf {
    let home = home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
    home.join(".fluxmirror")
}

/// Cache directory for downloaded binaries (used by wrappers).
pub fn cache_dir() -> PathBuf {
    let home = home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
    #[cfg(target_os = "windows")]
    {
        if let Some(local) = env::var_os("LOCALAPPDATA").filter(|s| !s.is_empty()) {
            return PathBuf::from(local).join("fluxmirror").join("cache");
        }
    }
    home.join(".fluxmirror/cache")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RAII guard that restores an env var to its prior value on drop —
    /// keeps tests in this crate from leaking env mutation into siblings.
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
    fn default_db_path_honors_env_override() {
        let _g = EnvGuard::set("FLUXMIRROR_DB", "/tmp/override.db");
        assert_eq!(default_db_path(), PathBuf::from("/tmp/override.db"));
    }

    #[test]
    fn home_dir_falls_back_to_userprofile() {
        let _h = EnvGuard::unset("HOME");
        let _u = EnvGuard::set("USERPROFILE", "/tmp/winhome");
        assert_eq!(home_dir(), Some(PathBuf::from("/tmp/winhome")));
    }

    #[test]
    fn home_dir_none_when_neither_set() {
        let _h = EnvGuard::unset("HOME");
        let _u = EnvGuard::unset("USERPROFILE");
        assert_eq!(home_dir(), None);
    }

    #[test]
    fn config_dir_uses_dot_fluxmirror_under_home() {
        let _h = EnvGuard::set("HOME", "/tmp/somehome");
        let _u = EnvGuard::unset("USERPROFILE");
        assert_eq!(config_dir(), PathBuf::from("/tmp/somehome/.fluxmirror"));
    }

    #[test]
    fn legacy_macos_db_path_is_stable() {
        let _h = EnvGuard::set("HOME", "/tmp/stablehome");
        let _u = EnvGuard::unset("USERPROFILE");
        assert_eq!(
            legacy_macos_db_path(),
            PathBuf::from("/tmp/stablehome/Library/Application Support/fluxmirror/events.db")
        );
    }
}
