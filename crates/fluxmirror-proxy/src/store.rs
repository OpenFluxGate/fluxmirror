use rusqlite::{params, Connection};
use std::path::Path;
use std::time::Duration;

pub struct Event {
    pub ts_ms: i64,
    pub direction: String, // "c2s" or "s2c"
    pub server_name: String,
    pub raw_bytes: Vec<u8>,
}

pub struct EventStore {
    conn: Connection,
}

impl EventStore {
    pub fn open(path: &Path) -> Result<Self, rusqlite::Error> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let conn = Connection::open(path)?;
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             CREATE TABLE IF NOT EXISTS events (
               id INTEGER PRIMARY KEY AUTOINCREMENT,
               ts_ms INTEGER NOT NULL,
               direction TEXT NOT NULL CHECK (direction IN ('c2s', 's2c')),
               method TEXT,
               message_json TEXT NOT NULL,
               server_name TEXT NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_events_ts ON events(ts_ms);",
        )?;
        eprintln!("[fluxmirror-proxy] event store opened: {}", path.display());
        Ok(EventStore { conn })
    }

    pub fn insert_batch(&mut self, batch: &[Event]) {
        if batch.is_empty() {
            return;
        }
        let tx = match self.conn.transaction() {
            Ok(t) => t,
            Err(e) => {
                eprintln!("[fluxmirror-proxy] WARN begin tx failed: {e}");
                return;
            }
        };
        {
            let mut stmt = match tx.prepare_cached(
                "INSERT INTO events (ts_ms, direction, method, message_json, server_name) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            ) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("[fluxmirror-proxy] WARN prepare failed: {e}");
                    return;
                }
            };
            for ev in batch {
                let message_json = String::from_utf8_lossy(&ev.raw_bytes).into_owned();
                let method = extract_method(&ev.raw_bytes);
                if let Err(e) = stmt.execute(params![
                    ev.ts_ms,
                    ev.direction,
                    method,
                    message_json,
                    ev.server_name,
                ]) {
                    eprintln!("[fluxmirror-proxy] WARN insert failed: {e}");
                }
            }
        }
        if let Err(e) = tx.commit() {
            eprintln!(
                "[fluxmirror-proxy] WARN commit failed (rolled back {} events): {e}",
                batch.len()
            );
        }
    }
}

fn extract_method(raw: &[u8]) -> Option<String> {
    let v: serde_json::Value = serde_json::from_slice(raw).ok()?;
    v.get("method").and_then(|m| m.as_str()).map(String::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_method_picks_string_field() {
        let raw = br#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{}}"#;
        assert_eq!(extract_method(raw), Some("tools/call".to_string()));
    }

    #[test]
    fn extract_method_returns_none_for_response() {
        let raw = br#"{"jsonrpc":"2.0","id":1,"result":{}}"#;
        assert_eq!(extract_method(raw), None);
    }

    #[test]
    fn extract_method_returns_none_for_invalid_json() {
        let raw = b"not json";
        assert_eq!(extract_method(raw), None);
    }

    #[test]
    fn open_then_batch_insert_roundtrip() {
        let tmp = tempfile_path("store-test");
        let mut s = EventStore::open(&tmp).unwrap();
        s.insert_batch(&[
            Event {
                ts_ms: 1_000,
                direction: "c2s".to_string(),
                server_name: "fs".to_string(),
                raw_bytes: br#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#
                    .to_vec(),
            },
            Event {
                ts_ms: 1_010,
                direction: "s2c".to_string(),
                server_name: "fs".to_string(),
                raw_bytes: br#"{"jsonrpc":"2.0","id":1,"result":{"capabilities":{}}}"#.to_vec(),
            },
        ]);

        let conn = Connection::open(&tmp).unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 2);
        let method: Option<String> = conn
            .query_row(
                "SELECT method FROM events WHERE direction='c2s'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(method, Some("initialize".to_string()));
        std::fs::remove_file(&tmp).ok();
    }

    fn tempfile_path(label: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        let pid = std::process::id();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        p.push(format!("fmproxy-{label}-{pid}-{nanos}.db"));
        p
    }
}
