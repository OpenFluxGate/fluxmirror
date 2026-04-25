// Timezone parsing + system inference.
//
// Wraps chrono-tz to keep callers free of its parse-error type.

use chrono_tz::Tz;
use std::env;

/// Parse an IANA timezone string (e.g. "Asia/Seoul"). Returns
/// `Error::BadTimezone` on failure so callers can surface a friendly
/// message.
pub fn parse_tz(s: &str) -> crate::Result<Tz> {
    s.parse::<Tz>()
        .map_err(|_| crate::Error::BadTimezone(s.to_string()))
}

/// Best-effort guess at the system timezone:
///   1. `TZ` env var if set and parseable
///   2. POSIX `/etc/localtime` symlink target ("…/zoneinfo/Asia/Seoul")
///   3. UTC fallback
pub fn infer_default_tz() -> Tz {
    if let Ok(v) = env::var("TZ") {
        if let Ok(tz) = v.parse::<Tz>() {
            return tz;
        }
    }
    #[cfg(unix)]
    {
        if let Ok(target) = std::fs::read_link("/etc/localtime") {
            if let Some(name) = target.to_string_lossy().split("zoneinfo/").nth(1) {
                if let Ok(tz) = name.parse::<Tz>() {
                    return tz;
                }
            }
        }
    }
    chrono_tz::UTC
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_tz_known() {
        let tz = parse_tz("Asia/Seoul").unwrap();
        assert_eq!(tz.name(), "Asia/Seoul");
    }

    #[test]
    fn parse_tz_utc() {
        let tz = parse_tz("UTC").unwrap();
        assert_eq!(tz.name(), "UTC");
    }

    #[test]
    fn parse_tz_unknown_errors() {
        let err = parse_tz("Garbage/Nope").unwrap_err();
        assert!(matches!(err, crate::Error::BadTimezone(_)));
    }

    #[test]
    fn infer_default_tz_yields_parseable_name() {
        // Whatever it picks, the result must round-trip through Tz parsing.
        let tz = infer_default_tz();
        let again = tz.name().parse::<Tz>();
        assert!(again.is_ok(), "tz name {:?} did not round-trip", tz.name());
    }
}
