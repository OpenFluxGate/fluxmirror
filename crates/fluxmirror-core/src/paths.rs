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

/// User config directory, per-OS:
///
///   macOS:        `~/.fluxmirror/`          (kept for compat with existing config readers)
///   Windows:      `%APPDATA%\fluxmirror\`   (falls back to `~/AppData/Roaming/fluxmirror/`)
///   Linux/other:  `${XDG_CONFIG_HOME}/fluxmirror/`
///                 (falls back to `~/.config/fluxmirror/`)
pub fn config_dir() -> PathBuf {
    let home = home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
    #[cfg(target_os = "macos")]
    {
        home.join(".fluxmirror")
    }
    #[cfg(target_os = "windows")]
    {
        if let Some(appdata) = env::var_os("APPDATA").filter(|s| !s.is_empty()) {
            return PathBuf::from(appdata).join("fluxmirror");
        }
        home.join("AppData/Roaming/fluxmirror")
    }
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    {
        if let Some(xdg) = env::var_os("XDG_CONFIG_HOME").filter(|s| !s.is_empty()) {
            return PathBuf::from(xdg).join("fluxmirror");
        }
        home.join(".config/fluxmirror")
    }
}

/// Legacy config directory: `~/.fluxmirror/` on any OS.
/// Used by `fluxmirror doctor` (STEP 8) to detect and warn about migrations
/// from pre-Phase-1 installations where config_dir was always `~/.fluxmirror`.
pub fn legacy_unix_config_dir() -> PathBuf {
    let home = home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
    home.join(".fluxmirror")
}

/// Cache directory for downloaded binaries (used by wrappers).
///
///   macOS:        `~/.fluxmirror/cache`
///   Windows:      `%LOCALAPPDATA%\fluxmirror\cache`
///                 (falls back to `~/AppData/Local/fluxmirror/cache`)
///   Linux/other:  `${XDG_CACHE_HOME}/fluxmirror`
///                 (falls back to `~/.cache/fluxmirror`)
pub fn cache_dir() -> PathBuf {
    let home = home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
    #[cfg(target_os = "macos")]
    {
        home.join(".fluxmirror/cache")
    }
    #[cfg(target_os = "windows")]
    {
        if let Some(local) = env::var_os("LOCALAPPDATA").filter(|s| !s.is_empty()) {
            return PathBuf::from(local).join("fluxmirror").join("cache");
        }
        home.join("AppData/Local/fluxmirror/cache")
    }
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    {
        if let Some(xdg) = env::var_os("XDG_CACHE_HOME").filter(|s| !s.is_empty()) {
            return PathBuf::from(xdg).join("fluxmirror");
        }
        home.join(".cache/fluxmirror")
    }
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
        let _lock = crate::test_lock::env_lock();
        let _g = EnvGuard::set("FLUXMIRROR_DB", "/tmp/override.db");
        assert_eq!(default_db_path(), PathBuf::from("/tmp/override.db"));
    }

    #[test]
    fn home_dir_falls_back_to_userprofile() {
        let _lock = crate::test_lock::env_lock();
        let _h = EnvGuard::unset("HOME");
        let _u = EnvGuard::set("USERPROFILE", "/tmp/winhome");
        assert_eq!(home_dir(), Some(PathBuf::from("/tmp/winhome")));
    }

    #[test]
    fn home_dir_none_when_neither_set() {
        let _lock = crate::test_lock::env_lock();
        let _h = EnvGuard::unset("HOME");
        let _u = EnvGuard::unset("USERPROFILE");
        assert_eq!(home_dir(), None);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn config_dir_macos_stays_dot_fluxmirror() {
        let _lock = crate::test_lock::env_lock();
        let _h = EnvGuard::set("HOME", "/tmp/somehome");
        let _u = EnvGuard::unset("USERPROFILE");
        assert_eq!(config_dir(), PathBuf::from("/tmp/somehome/.fluxmirror"));
    }

    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    #[test]
    fn config_dir_linux_falls_back_to_dot_config() {
        let _lock = crate::test_lock::env_lock();
        let _h = EnvGuard::set("HOME", "/tmp/linuxhome");
        let _u = EnvGuard::unset("USERPROFILE");
        let _x = EnvGuard::unset("XDG_CONFIG_HOME");
        assert_eq!(
            config_dir(),
            PathBuf::from("/tmp/linuxhome/.config/fluxmirror")
        );
    }

    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    #[test]
    fn config_dir_linux_honors_xdg_config_home() {
        let _lock = crate::test_lock::env_lock();
        let _h = EnvGuard::set("HOME", "/tmp/linuxhome");
        let _u = EnvGuard::unset("USERPROFILE");
        let _x = EnvGuard::set("XDG_CONFIG_HOME", "/custom/config");
        assert_eq!(config_dir(), PathBuf::from("/custom/config/fluxmirror"));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn config_dir_windows_uses_appdata() {
        let _lock = crate::test_lock::env_lock();
        let _a = EnvGuard::set("APPDATA", r"C:\Users\test\AppData\Roaming");
        assert_eq!(
            config_dir(),
            PathBuf::from(r"C:\Users\test\AppData\Roaming\fluxmirror")
        );
    }

    #[test]
    fn legacy_unix_config_dir_always_dot_fluxmirror() {
        let _lock = crate::test_lock::env_lock();
        let _h = EnvGuard::set("HOME", "/tmp/anyhome");
        let _u = EnvGuard::unset("USERPROFILE");
        assert_eq!(
            legacy_unix_config_dir(),
            PathBuf::from("/tmp/anyhome/.fluxmirror")
        );
    }

    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    #[test]
    fn cache_dir_linux_falls_back_to_dot_cache() {
        let _lock = crate::test_lock::env_lock();
        let _h = EnvGuard::set("HOME", "/tmp/linuxhome");
        let _u = EnvGuard::unset("USERPROFILE");
        let _x = EnvGuard::unset("XDG_CACHE_HOME");
        assert_eq!(
            cache_dir(),
            PathBuf::from("/tmp/linuxhome/.cache/fluxmirror")
        );
    }

    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    #[test]
    fn cache_dir_linux_honors_xdg_cache_home() {
        let _lock = crate::test_lock::env_lock();
        let _h = EnvGuard::set("HOME", "/tmp/linuxhome");
        let _u = EnvGuard::unset("USERPROFILE");
        let _x = EnvGuard::set("XDG_CACHE_HOME", "/custom/cache");
        assert_eq!(cache_dir(), PathBuf::from("/custom/cache/fluxmirror"));
    }

    #[test]
    fn legacy_macos_db_path_is_stable() {
        let _lock = crate::test_lock::env_lock();
        let _h = EnvGuard::set("HOME", "/tmp/stablehome");
        let _u = EnvGuard::unset("USERPROFILE");
        assert_eq!(
            legacy_macos_db_path(),
            PathBuf::from("/tmp/stablehome/Library/Application Support/fluxmirror/events.db")
        );
    }
}
