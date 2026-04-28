// Integration smoke tests for `fluxmirror upgrade`.
//
// Each test spins up a tiny in-process HTTP server (std::net,
// no extra deps), seeds it with a fake `releases/latest` payload plus
// fake binary + .sha256 assets, then invokes the real `fluxmirror`
// binary with FLUXMIRROR_UPGRADE_API_BASE pointed at the mock server
// and FLUXMIRROR_UPGRADE_TARGET_OVERRIDE pointed at a writable temp
// path so we never overwrite the test runner.
//
// Coverage:
//   - happy path: download + verify + atomic swap
//   - sha256 mismatch: existing binary stays untouched, exit non-zero
//   - already on latest: noop, exit 0, binary unchanged
//   - missing arch asset: graceful failure, binary unchanged

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::Command;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread;
use std::time::Duration;

use sha2::{Digest, Sha256};
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// host-arch asset naming — must match cmd/upgrade.rs
// ---------------------------------------------------------------------------

fn host_asset_suffix() -> &'static str {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "darwin-arm64"
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        "darwin-x64"
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        "linux-x64"
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        "linux-arm64"
    } else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        "windows-x64"
    } else {
        "unsupported"
    }
}

fn host_bin_ext() -> &'static str {
    if cfg!(target_os = "windows") {
        ".exe"
    } else {
        ""
    }
}

// ---------------------------------------------------------------------------
// mock HTTP server
// ---------------------------------------------------------------------------

struct MockServer {
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl Drop for MockServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

fn handle_request(mut stream: TcpStream, handler: &dyn Fn(&str) -> (u16, Vec<u8>)) {
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() {
        return;
    }
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h).is_err() {
            return;
        }
        if h == "\r\n" || h == "\n" || h.is_empty() {
            break;
        }
    }
    let path = request_line.split_whitespace().nth(1).unwrap_or("/");
    let (status, body) = handler(path);
    let reason = match status {
        200 => "OK",
        404 => "Not Found",
        _ => "OK",
    };
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(&body);
    let _ = stream.flush();
}

/// Bind 127.0.0.1 on the given port, retrying briefly while the OS
/// releases the just-freed port. Returns a server that handles every
/// request the four upgrade tests can issue:
///   - GET /repos/.../releases/latest -> release_json
///   - GET /dl/bin                    -> bin_bytes
///   - GET /dl/bin.sha256             -> sha_text
///   - anything else                  -> 404
fn start_mock_server_on_port(
    port: u16,
    release_json: String,
    bin_bytes: Vec<u8>,
    sha_text: String,
) -> MockServer {
    let bind_addr = format!("127.0.0.1:{port}");
    let mut listener_opt: Option<TcpListener> = None;
    for _ in 0..40 {
        match TcpListener::bind(&bind_addr) {
            Ok(l) => {
                listener_opt = Some(l);
                break;
            }
            Err(_) => thread::sleep(Duration::from_millis(25)),
        }
    }
    let listener = listener_opt.expect("rebind ephemeral port for mock server");
    listener.set_nonblocking(true).ok();

    let release_json = Arc::new(release_json);
    let bin_bytes = Arc::new(bin_bytes);
    let sha_text = Arc::new(sha_text);
    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = stop.clone();

    let handle = thread::spawn(move || {
        while !stop_thread.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((stream, _)) => {
                    stream.set_nonblocking(false).ok();
                    let release_json = release_json.clone();
                    let bin_bytes = bin_bytes.clone();
                    let sha_text = sha_text.clone();
                    thread::spawn(move || {
                        handle_request(stream, &move |path: &str| {
                            if path.contains("/releases/latest") {
                                (200, release_json.as_bytes().to_vec())
                            } else if path == "/dl/bin" {
                                (200, bin_bytes.to_vec())
                            } else if path == "/dl/bin.sha256" {
                                (200, sha_text.as_bytes().to_vec())
                            } else {
                                (404, b"not found".to_vec())
                            }
                        });
                    });
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(20));
                }
                Err(_) => break,
            }
        }
    });

    MockServer {
        stop,
        handle: Some(handle),
    }
}

// ---------------------------------------------------------------------------
// fixtures
// ---------------------------------------------------------------------------

struct Fixture {
    _tmp: TempDir,
    target_path: PathBuf,
    bin_asset_name: String,
    sha_asset_name: String,
    new_bin_bytes: Vec<u8>,
    new_bin_sha: String,
}

fn make_fixture(old_content: &[u8], new_content: Vec<u8>) -> Fixture {
    let tmp = tempfile::tempdir().expect("tempdir");
    let ext = host_bin_ext();
    let target = tmp.path().join(format!("fluxmirror-fake{ext}"));
    std::fs::write(&target, old_content).unwrap();

    let suffix = host_asset_suffix();
    let bin_asset_name = format!("fluxmirror-{suffix}{ext}");
    let sha_asset_name = format!("{bin_asset_name}.sha256");
    let sha = sha256_hex(&new_content);

    Fixture {
        _tmp: tmp,
        target_path: target,
        bin_asset_name,
        sha_asset_name,
        new_bin_bytes: new_content,
        new_bin_sha: sha,
    }
}

fn release_json_for(base_url: &str, tag: &str, bin_name: &str, sha_name: &str) -> String {
    serde_json::json!({
        "tag_name": tag,
        "assets": [
            { "name": bin_name,
              "browser_download_url": format!("{base_url}/dl/bin"),
              "digest": null },
            { "name": sha_name,
              "browser_download_url": format!("{base_url}/dl/bin.sha256"),
              "digest": null },
        ],
    })
    .to_string()
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    let digest = h.finalize();
    let mut s = String::with_capacity(64);
    for b in digest {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

fn pick_port() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").expect("bind preview");
    let port = l.local_addr().unwrap().port();
    drop(l);
    port
}

fn run_fluxmirror_upgrade(
    api_base: &str,
    target_path: &std::path::Path,
    extra_args: &[&str],
) -> std::process::Output {
    let bin = env!("CARGO_BIN_EXE_fluxmirror");
    let mut cmd = Command::new(bin);
    cmd.arg("upgrade")
        .args(extra_args)
        .env("FLUXMIRROR_UPGRADE_API_BASE", api_base)
        .env("FLUXMIRROR_UPGRADE_TARGET_OVERRIDE", target_path);
    cmd.output().expect("invoke fluxmirror upgrade")
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

#[test]
fn happy_path_downloads_and_swaps() {
    let fx = make_fixture(b"OLD-BINARY", b"NEW-BINARY-CONTENT-12345".to_vec());

    let port = pick_port();
    let base_url = format!("http://127.0.0.1:{port}");
    let release_json =
        release_json_for(&base_url, "v9.9.9", &fx.bin_asset_name, &fx.sha_asset_name);
    let sha_text = format!("{}  fake-binary\n", fx.new_bin_sha);

    let server =
        start_mock_server_on_port(port, release_json, fx.new_bin_bytes.clone(), sha_text);
    let out = run_fluxmirror_upgrade(&base_url, &fx.target_path, &[]);
    drop(server);

    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(
        out.status.success(),
        "expected success; stdout={stdout}, stderr={stderr}"
    );

    let after = std::fs::read(&fx.target_path).unwrap();
    assert_eq!(
        after, fx.new_bin_bytes,
        "binary should have been swapped to the new content"
    );
}

#[test]
fn sha_mismatch_leaves_binary_untouched() {
    let old = b"UNCHANGED-AFTER-SHA-MISMATCH";
    let fx = make_fixture(old, b"WOULD-BE-NEW".to_vec());

    let port = pick_port();
    let base_url = format!("http://127.0.0.1:{port}");
    let release_json =
        release_json_for(&base_url, "v9.9.9", &fx.bin_asset_name, &fx.sha_asset_name);
    // Wrong digest: hash of unrelated content, never matches new_bin_bytes.
    let wrong_sha = sha256_hex(b"DIFFERENT-CONTENT");
    let sha_text = format!("{wrong_sha}  fake-binary\n");

    let server =
        start_mock_server_on_port(port, release_json, fx.new_bin_bytes.clone(), sha_text);
    let out = run_fluxmirror_upgrade(&base_url, &fx.target_path, &[]);
    drop(server);

    assert!(
        !out.status.success(),
        "expected non-zero exit on sha mismatch"
    );
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(
        stderr.contains("sha256 mismatch"),
        "expected sha mismatch in stderr, got: {stderr}"
    );

    let after = std::fs::read(&fx.target_path).unwrap();
    assert_eq!(after, old, "binary must stay intact on sha mismatch");
}

#[test]
fn already_on_latest_no_op() {
    let old = b"UNCHANGED-AT-LATEST";
    let fx = make_fixture(old, b"WOULD-BE-NEW".to_vec());

    let port = pick_port();
    let base_url = format!("http://127.0.0.1:{port}");
    let cur = env!("CARGO_PKG_VERSION");
    let release_json = release_json_for(
        &base_url,
        &format!("v{cur}"),
        &fx.bin_asset_name,
        &fx.sha_asset_name,
    );
    let sha_text = format!("{}  fake-binary\n", fx.new_bin_sha);

    let server =
        start_mock_server_on_port(port, release_json, fx.new_bin_bytes.clone(), sha_text);
    let out = run_fluxmirror_upgrade(&base_url, &fx.target_path, &[]);
    drop(server);

    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(
        out.status.success(),
        "expected exit 0 when already on latest; stdout={stdout}"
    );
    assert!(
        stdout.contains("already on latest"),
        "expected 'already on latest' in stdout, got: {stdout}"
    );

    let after = std::fs::read(&fx.target_path).unwrap();
    assert_eq!(after, old, "binary must stay intact when already on latest");
}

#[test]
fn missing_asset_for_arch_is_clean_error() {
    let old = b"UNCHANGED-NO-ASSET";
    let fx = make_fixture(old, b"WOULD-BE-NEW".to_vec());

    let port = pick_port();
    let base_url = format!("http://127.0.0.1:{port}");
    let release_json = serde_json::json!({
        "tag_name": "v9.9.9",
        "assets": [
            { "name": "fluxmirror-some-other-arch",
              "browser_download_url": format!("{base_url}/dl/bin"),
              "digest": null },
        ],
    })
    .to_string();
    let sha_text = format!("{}  fake-binary\n", fx.new_bin_sha);

    let server =
        start_mock_server_on_port(port, release_json, fx.new_bin_bytes.clone(), sha_text);
    let out = run_fluxmirror_upgrade(&base_url, &fx.target_path, &[]);
    drop(server);

    assert!(
        !out.status.success(),
        "expected non-zero exit when arch asset is missing"
    );
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(
        stderr.contains("missing asset"),
        "expected 'missing asset' in stderr, got: {stderr}"
    );

    let after = std::fs::read(&fx.target_path).unwrap();
    assert_eq!(after, old, "binary must stay intact on missing-asset error");
}
