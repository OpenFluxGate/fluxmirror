// Shared helpers for the report-style subcommands (window / histogram /
// daily-totals / per-day-files / sqlite).
//
// All errors here are user-facing, not telemetry: the report subcommands
// are run synchronously by slash commands and can fail loudly. Callers
// that want a non-zero exit code on bad input use `print_err_exit2()`.
//
// We deliberately keep these functions free of I/O side effects beyond
// what each one's name advertises.

use std::path::Path;
use std::process::ExitCode;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use rusqlite::{Connection, OpenFlags};

/// Parse an ISO 8601 UTC timestamp ending in `Z`. Accepts second
/// precision (`2026-04-26T12:34:56Z`) and fractional seconds
/// (`2026-04-26T12:34:56.123Z`); both round-trip through chrono.
///
/// Returns a one-line human-friendly error string on failure.
pub fn parse_iso8601_z(s: &str) -> Result<DateTime<Utc>, String> {
    // chrono's RFC 3339 parser accepts both Z and ±HH:MM offsets, plus
    // optional fractional seconds. We force-convert to UTC after parse.
    match DateTime::parse_from_rfc3339(s) {
        Ok(dt) => Ok(dt.with_timezone(&Utc)),
        Err(e) => Err(format!("invalid ISO 8601 UTC timestamp {s:?}: {e}")),
    }
}

/// Parse an IANA timezone string (e.g. `Asia/Seoul`, `UTC`).
pub fn parse_tz(s: &str) -> Result<Tz, String> {
    Tz::from_str(s).map_err(|e| format!("invalid timezone {s:?}: {e}"))
}

/// Open a SQLite database read-only. Fails if the file does not exist —
/// callers should surface that to the user. Read-only is safe for the
/// report subcommands: none of them mutate the DB.
pub fn open_db_readonly(path: &Path) -> Result<Connection, String> {
    if !path.exists() {
        return Err(format!("database not found: {}", path.display()));
    }
    Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| format!("open db {}: {e}", path.display()))
}

/// Open a SQLite database read/write (used only by `sqlite` subcommand
/// since the user may pass an INSERT/UPDATE/etc.). Creates parent dirs
/// implicitly only when the file already exists at a valid path — we
/// never auto-create the DB here.
pub fn open_db_readwrite(path: &Path) -> Result<Connection, String> {
    if !path.exists() {
        return Err(format!("database not found: {}", path.display()));
    }
    Connection::open(path).map_err(|e| format!("open db {}: {e}", path.display()))
}

/// Print an error to stderr and return exit code 2. Used by all report
/// subcommands to keep the call sites at the bottom of each `run()`
/// readable.
pub fn err_exit2(msg: impl AsRef<str>) -> ExitCode {
    eprintln!("{}", msg.as_ref());
    ExitCode::from(2)
}

#[cfg(test)]
pub mod test_helpers {
    //! Shared test plumbing: a single global mutex that every cmd test
    //! module locks before mutating environment variables. Tests in
    //! different modules run in the same process and on the same env
    //! — without one shared lock, parallel tests can read each other's
    //! HOME / FLUXMIRROR_DB / LANG / etc.
    use std::sync::{Mutex, MutexGuard};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    pub fn env_lock() -> MutexGuard<'static, ()> {
        ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub struct EnvGuard {
        key: &'static str,
        prior: Option<std::ffi::OsString>,
    }
    impl EnvGuard {
        pub fn set(key: &'static str, value: &str) -> Self {
            let prior = std::env::var_os(key);
            std::env::set_var(key, value);
            Self { key, prior }
        }
        pub fn unset(key: &'static str) -> Self {
            let prior = std::env::var_os(key);
            std::env::remove_var(key);
            Self { key, prior }
        }
    }
    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match self.prior.take() {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_iso8601_z_accepts_seconds() {
        let dt = parse_iso8601_z("2026-04-26T12:34:56Z").unwrap();
        assert_eq!(dt.to_rfc3339(), "2026-04-26T12:34:56+00:00");
    }

    #[test]
    fn parse_iso8601_z_accepts_fractional() {
        let dt = parse_iso8601_z("2026-04-26T12:34:56.789Z").unwrap();
        // Fractional seconds preserved.
        assert_eq!(
            dt.timestamp_subsec_millis(),
            789,
            "fractional seconds should round-trip"
        );
    }

    #[test]
    fn parse_iso8601_z_rejects_garbage() {
        assert!(parse_iso8601_z("not-a-date").is_err());
        assert!(parse_iso8601_z("2026-13-01T00:00:00Z").is_err());
    }

    #[test]
    fn parse_tz_accepts_known() {
        assert_eq!(parse_tz("Asia/Seoul").unwrap().name(), "Asia/Seoul");
        assert_eq!(parse_tz("UTC").unwrap().name(), "UTC");
    }

    #[test]
    fn parse_tz_rejects_unknown() {
        assert!(parse_tz("Atlantis/Lost").is_err());
    }
}
