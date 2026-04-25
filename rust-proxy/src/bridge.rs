use crate::framer::Framer;
use crate::store::Event;
use std::fs::File;
use std::io::{Read, Write};
use std::sync::mpsc::Sender;
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

const BUFFER_SIZE: usize = 8192;
const LOG_TRUNCATE_CHARS: usize = 2000;

/// Spawn the two relay threads. They run until their source closes
/// (EOF) or hits an error. Returns the join handles so the caller can
/// wait for both to complete.
pub fn run<RIn: Read + Send + 'static, WOut: Write + Send + 'static,
           RChild: Read + Send + 'static, WChild: Write + Send + 'static>(
    parent_in: RIn,
    parent_out: WOut,
    child_in: WChild,
    child_out: RChild,
    capture_c2s: Option<File>,
    capture_s2c: Option<File>,
    tx: Sender<Event>,
    server_name: String,
) -> (thread::JoinHandle<()>, thread::JoinHandle<()>) {
    let server_name_c2s = server_name.clone();
    let tx_c2s = tx.clone();
    let c2s = thread::Builder::new()
        .name("c2s".to_string())
        .spawn(move || {
            relay(
                parent_in,
                child_in,
                "c2s",
                capture_c2s,
                tx_c2s,
                server_name_c2s,
            );
        })
        .expect("spawn c2s");

    let s2c = thread::Builder::new()
        .name("s2c".to_string())
        .spawn(move || {
            relay(
                child_out,
                parent_out,
                "s2c",
                capture_s2c,
                tx,
                server_name,
            );
        })
        .expect("spawn s2c");

    eprintln!("[fluxmirror-proxy] relay started");
    (c2s, s2c)
}

fn relay<R: Read, W: Write>(
    mut src: R,
    mut dst: W,
    direction: &'static str,
    mut capture: Option<File>,
    tx: Sender<Event>,
    server_name: String,
) {
    let mut buf = vec![0u8; BUFFER_SIZE];
    let mut framer = Framer::new();
    let mut capture_failed = false;
    let mut tx_full = false;

    loop {
        let n = match src.read(&mut buf) {
            Ok(0) => break, // EOF
            Ok(n) => n,
            Err(e) => {
                eprintln!("[fluxmirror-proxy] DEBUG relay {direction} read error: {e}");
                break;
            }
        };

        // 1. Relay — absolute priority. If this fails, abort the relay.
        if let Err(e) = dst.write_all(&buf[..n]) {
            eprintln!("[fluxmirror-proxy] WARN relay {direction} write failed: {e}");
            break;
        }
        let _ = dst.flush();

        // 2. Capture — best-effort, disable on first error.
        if !capture_failed {
            if let Some(file) = capture.as_mut() {
                if let Err(e) = file.write_all(&buf[..n]).and_then(|_| file.flush()) {
                    capture_failed = true;
                    eprintln!(
                        "[fluxmirror-proxy] WARN capture {direction} write failed, disabling: {e}"
                    );
                }
            }
        }

        // 3. Framer + log + event emission — best-effort, never blocks the relay.
        let messages = framer.feed(&buf[..n]);
        for msg in messages {
            log_message(direction, &msg);
            let event = Event {
                ts_ms: now_ms(),
                direction: direction.to_string(),
                server_name: server_name.clone(),
                raw_bytes: msg,
            };
            if tx.send(event).is_err() && !tx_full {
                tx_full = true;
                eprintln!(
                    "[fluxmirror-proxy] WARN event channel closed, dropping events for direction={direction}"
                );
            }
        }
    }
    eprintln!("[fluxmirror-proxy] relay stopped direction={direction}");
}

fn log_message(direction: &str, msg: &[u8]) {
    let text = String::from_utf8_lossy(msg);
    if text.chars().count() > LOG_TRUNCATE_CHARS {
        let truncated: String = text.chars().take(LOG_TRUNCATE_CHARS).collect();
        eprintln!(
            "[fluxmirror-proxy] [{direction}] {truncated}... ({} bytes total)",
            msg.len()
        );
    } else {
        eprintln!("[fluxmirror-proxy] [{direction}] {text}");
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
