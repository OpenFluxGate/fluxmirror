// SQLite-backed response cache.
//
// Schema lives in `fluxmirror-store::SqliteStore::migrate()`. Lookups
// apply a TTL on read; sweeps run on a write path that exceeds a
// "stale-row threshold" to keep the table from growing without bound
// over weeks of use.
//
// Cache key contract (from `synthesise()`):
//
//   sha256_hex(model + "\x00" + system + "\x00" + redacted_user + "\x00" + version)
//
// Including the prompt-version digest in the key means bumping a
// prompt's `# version: N` line invalidates every cached response that
// went through the old template.

use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, OptionalExtension};
use sha2::{Digest, Sha256};

use crate::types::{AiError, LlmResponse};

/// Stable hex digest used as the `ai_cache.key` primary key.
pub fn make_cache_key(model: &str, system: &str, redacted_user: &str, version: u32) -> String {
    let mut h = Sha256::new();
    h.update(model.as_bytes());
    h.update(b"\x00");
    h.update(system.as_bytes());
    h.update(b"\x00");
    h.update(redacted_user.as_bytes());
    h.update(b"\x00");
    h.update(version.to_string().as_bytes());
    let digest = h.finalize();
    let mut out = String::with_capacity(64);
    for b in digest.iter() {
        use std::fmt::Write;
        let _ = write!(out, "{b:02x}");
    }
    out
}

/// Look up a cached response. Returns `None` on a miss or a TTL eviction.
/// On hit the row is returned as an `LlmResponse` with `cache_hit = true`,
/// `cost_usd = 0.0`, and `tokens_in = tokens_out = 0` (real spend was
/// already booked on the original call).
pub fn lookup(
    conn: &rusqlite::Connection,
    key: &str,
    ttl_days: u32,
) -> Result<Option<LlmResponse>, AiError> {
    let row = conn
        .query_row(
            "SELECT response, created_at, model, provider FROM ai_cache WHERE key = ?1",
            params![key],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            },
        )
        .optional()?;
    let (response, created_at, model, provider_owned) = match row {
        Some(t) => t,
        None => return Ok(None),
    };
    let now = epoch_seconds();
    if now > created_at && (now - created_at) > (ttl_days as i64 * 86_400) {
        // Stale — drop and miss.
        let _ = conn.execute("DELETE FROM ai_cache WHERE key = ?1", params![key]);
        return Ok(None);
    }
    Ok(Some(LlmResponse {
        text: response,
        model,
        provider: static_provider_label(&provider_owned),
        cost_usd: 0.0,
        tokens_in: 0,
        tokens_out: 0,
        cache_hit: true,
    }))
}

/// Write a response to the cache. `model` and `provider` are stored as
/// independent columns so the studio's "cache hit-rate by model" view
/// can group without re-parsing the response body.
pub fn insert(
    conn: &rusqlite::Connection,
    key: &str,
    response_text: &str,
    cost_usd: f64,
    model: &str,
    provider: &str,
) -> Result<(), AiError> {
    let now = epoch_seconds();
    conn.execute(
        "INSERT OR REPLACE INTO ai_cache (key, response, created_at, cost_usd, model, provider) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![key, response_text, now, cost_usd, model, provider],
    )?;
    Ok(())
}

fn epoch_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Map a stored provider name back to the &'static str the LlmResponse
/// type wants. Anything we don't recognise becomes `"unknown"` so the
/// type stays `&'static`.
fn static_provider_label(name: &str) -> &'static str {
    match name {
        "anthropic" => "anthropic",
        "ollama" => "ollama",
        "off" => "off",
        _ => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fluxmirror_store::SqliteStore;
    use rusqlite::Connection;

    fn open_store(dir: &std::path::Path) -> (Connection, SqliteStore) {
        let path = dir.join("events.db");
        let store = SqliteStore::open(&path).expect("store opens");
        let conn = Connection::open(&path).expect("raw conn");
        (conn, store)
    }

    #[test]
    fn key_is_deterministic_and_version_sensitive() {
        let a = make_cache_key("m", "s", "u", 1);
        let b = make_cache_key("m", "s", "u", 1);
        let c = make_cache_key("m", "s", "u", 2);
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(a.len(), 64); // sha256 hex
    }

    #[test]
    fn insert_then_lookup_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let (conn, _s) = open_store(dir.path());
        let key = make_cache_key("claude-haiku-4-5", "sys", "user", 1);
        insert(&conn, &key, "the response", 0.0001, "claude-haiku-4-5", "anthropic").unwrap();
        let hit = lookup(&conn, &key, 7).unwrap().expect("hit");
        assert!(hit.cache_hit);
        assert_eq!(hit.text, "the response");
        assert_eq!(hit.cost_usd, 0.0);
        assert_eq!(hit.provider, "anthropic");
    }

    #[test]
    fn ttl_eviction_returns_miss() {
        let dir = tempfile::tempdir().unwrap();
        let (conn, _s) = open_store(dir.path());
        // Hand-write an aged row.
        conn.execute(
            "INSERT INTO ai_cache (key, response, created_at, cost_usd, model, provider) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params!["aged", "old", 0i64, 0.001f64, "claude-haiku-4-5", "anthropic"],
        )
        .unwrap();
        assert!(lookup(&conn, "aged", 7).unwrap().is_none());
    }
}
