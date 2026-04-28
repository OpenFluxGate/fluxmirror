// fluxmirror upgrade — self-update from GitHub Releases.
//
// Polls the GitHub Releases API for the latest tag, downloads the
// binary asset matching the current arch, verifies SHA256 against a
// sibling .sha256 asset, and atomically swaps the on-disk binary via
// tempfile::NamedTempFile::persist (POSIX rename semantics).
//
// With --with-studio, also updates fluxmirror-studio when it is
// installed alongside fluxmirror or available on PATH.
//
// Permission errors print an actionable sudo hint instead of shelling
// out themselves. SHA mismatch leaves the existing binary untouched.
//
// HTTP transport is ureq with rustls (no openssl) so the binary stays
// statically-linked-friendly.
//
// Test-only env overrides:
//   FLUXMIRROR_UPGRADE_API_BASE              — replaces https://api.github.com
//   FLUXMIRROR_UPGRADE_TARGET_OVERRIDE       — replaces env::current_exe()
//   FLUXMIRROR_UPGRADE_STUDIO_TARGET_OVERRIDE — replaces studio path lookup

use std::env;
use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use serde_json::Value;
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;

const API_BASE_DEFAULT: &str = "https://api.github.com";
const REPO_DEFAULT: &str = "OpenFluxGate/fluxmirror";

/// Command-line surface for `fluxmirror upgrade`.
pub struct UpgradeArgs {
    pub with_studio: bool,
    pub dry_run: bool,
    pub asset_repo: Option<String>,
    pub current_version: String,
}

pub fn run(args: UpgradeArgs) -> ExitCode {
    match run_inner(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(UpgradeError::PermissionDenied) => ExitCode::from(2),
        Err(UpgradeError::Other(msg)) => {
            eprintln!("fluxmirror upgrade: {msg}");
            ExitCode::from(2)
        }
    }
}

enum UpgradeError {
    PermissionDenied,
    Other(String),
}

impl From<String> for UpgradeError {
    fn from(s: String) -> Self {
        UpgradeError::Other(s)
    }
}

fn run_inner(args: UpgradeArgs) -> Result<(), UpgradeError> {
    let repo = args
        .asset_repo
        .clone()
        .unwrap_or_else(|| REPO_DEFAULT.to_string());
    let target = current_target_triple();
    let asset_suffix = asset_suffix_for_target(target).ok_or_else(|| {
        UpgradeError::Other(format!(
            "no upgrade asset published for target triple {target}"
        ))
    })?;
    let ext = bin_ext();

    let api_base = env::var("FLUXMIRROR_UPGRADE_API_BASE")
        .unwrap_or_else(|_| API_BASE_DEFAULT.to_string());
    let url = format!("{api_base}/repos/{repo}/releases/latest");

    let release = fetch_release_json(&url, &args.current_version)?;
    let tag_name = release
        .get("tag_name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| UpgradeError::Other("release JSON missing tag_name".to_string()))?
        .to_string();

    if !is_newer_semver(&tag_name, &args.current_version) {
        println!(
            "fluxmirror upgrade: already on latest ({})",
            strip_v_prefix(&args.current_version)
        );
        return Ok(());
    }

    let assets = release
        .get("assets")
        .and_then(|v| v.as_array())
        .ok_or_else(|| UpgradeError::Other("release JSON missing assets[]".to_string()))?
        .clone();

    let exe = resolve_target_path()?;

    let mut targets: Vec<(String, PathBuf)> = Vec::new();
    targets.push(("fluxmirror".to_string(), exe.clone()));

    if args.with_studio {
        match resolve_studio_path(&exe) {
            Some(p) => targets.push(("fluxmirror-studio".to_string(), p)),
            None => println!(
                "fluxmirror upgrade: fluxmirror-studio not found alongside or on PATH; skipping"
            ),
        }
    }

    for (name, path) in &targets {
        let asset_name = format!("{name}-{asset_suffix}{ext}");
        let sha_name = format!("{asset_name}.sha256");
        let bin_url = find_asset_url(&assets, &asset_name).ok_or_else(|| {
            UpgradeError::Other(format!("release {tag_name} missing asset {asset_name}"))
        })?;
        let sha_url = find_asset_url(&assets, &sha_name).ok_or_else(|| {
            UpgradeError::Other(format!("release {tag_name} missing asset {sha_name}"))
        })?;

        let parent = path.parent().ok_or_else(|| {
            UpgradeError::Other(format!("cannot resolve parent dir of {}", path.display()))
        })?;

        let mut tmp = match NamedTempFile::new_in(parent) {
            Ok(t) => t,
            Err(e) if e.kind() == ErrorKind::PermissionDenied => {
                let with_studio_flag = if args.with_studio { " --with-studio" } else { "" };
                eprintln!(
                    "fluxmirror upgrade: cannot write to {} (permission denied).",
                    path.display()
                );
                eprintln!("Re-run as: sudo fluxmirror upgrade{with_studio_flag}");
                return Err(UpgradeError::PermissionDenied);
            }
            Err(e) => {
                return Err(UpgradeError::Other(format!(
                    "create tempfile in {}: {e}",
                    parent.display()
                )));
            }
        };

        let bytes = http_get_bytes(&bin_url, &args.current_version)?;
        tmp.write_all(&bytes)
            .map_err(|e| format!("write tempfile: {e}"))?;
        tmp.flush()
            .map_err(|e| format!("flush tempfile: {e}"))?;

        let sha_text = http_get_text(&sha_url, &args.current_version)?;
        let expected = parse_sha256_line(&sha_text);
        if expected.is_empty() {
            return Err(UpgradeError::Other(format!(
                "empty .sha256 for {asset_name}; existing binary at {} unchanged",
                path.display()
            )));
        }
        let actual = sha256_hex(&bytes);
        if !actual.eq_ignore_ascii_case(&expected) {
            return Err(UpgradeError::Other(format!(
                "sha256 mismatch for {asset_name}: expected {expected}, got {actual}; \
                 existing binary at {} unchanged",
                path.display()
            )));
        }

        if args.dry_run {
            println!(
                "fluxmirror upgrade: dry-run ok — would replace {} with {asset_name} ({tag_name})",
                path.display()
            );
            continue;
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(tmp.path())
                .map_err(|e| format!("stat tmp: {e}"))?
                .permissions();
            perms.set_mode(0o755);
            fs::set_permissions(tmp.path(), perms)
                .map_err(|e| format!("chmod tmp: {e}"))?;
        }

        tmp.persist(path)
            .map_err(|e| format!("rename {}: {e}", path.display()))?;

        println!(
            "fluxmirror upgrade: {} -> {tag_name}",
            path.display()
        );
    }

    Ok(())
}

fn resolve_target_path() -> Result<PathBuf, UpgradeError> {
    if let Ok(p) = env::var("FLUXMIRROR_UPGRADE_TARGET_OVERRIDE") {
        return Ok(PathBuf::from(p));
    }
    let exe = env::current_exe()
        .map_err(|e| UpgradeError::Other(format!("current_exe: {e}")))?;
    Ok(exe.canonicalize().unwrap_or(exe))
}

fn resolve_studio_path(fluxmirror_path: &Path) -> Option<PathBuf> {
    if let Ok(p) = env::var("FLUXMIRROR_UPGRADE_STUDIO_TARGET_OVERRIDE") {
        let pb = PathBuf::from(p);
        return if pb.exists() { Some(pb) } else { None };
    }
    let ext = bin_ext();
    if let Some(parent) = fluxmirror_path.parent() {
        let candidate = parent.join(format!("fluxmirror-studio{ext}"));
        if candidate.exists() {
            return Some(candidate);
        }
    }
    let path_var = env::var_os("PATH")?;
    let separator = if cfg!(target_os = "windows") { ';' } else { ':' };
    for dir in path_var.to_string_lossy().split(separator) {
        if dir.is_empty() {
            continue;
        }
        let p = Path::new(dir).join(format!("fluxmirror-studio{ext}"));
        if p.exists() {
            return Some(p);
        }
    }
    None
}

fn current_target_triple() -> &'static str {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "aarch64-apple-darwin"
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        "x86_64-apple-darwin"
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        "x86_64-unknown-linux-gnu"
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        "aarch64-unknown-linux-gnu"
    } else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        "x86_64-pc-windows-msvc"
    } else {
        "unsupported"
    }
}

fn asset_suffix_for_target(triple: &str) -> Option<&'static str> {
    match triple {
        "aarch64-apple-darwin" => Some("darwin-arm64"),
        "x86_64-apple-darwin" => Some("darwin-x64"),
        "x86_64-unknown-linux-gnu" => Some("linux-x64"),
        "aarch64-unknown-linux-gnu" => Some("linux-arm64"),
        "x86_64-pc-windows-msvc" => Some("windows-x64"),
        _ => None,
    }
}

fn bin_ext() -> &'static str {
    if cfg!(target_os = "windows") {
        ".exe"
    } else {
        ""
    }
}

fn build_agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(60))
        .build()
}

fn fetch_release_json(url: &str, current_version: &str) -> Result<Value, UpgradeError> {
    let agent = build_agent();
    let user_agent = format!("fluxmirror/{current_version}");
    let mut last_err = String::new();
    // 1 retry with exponential backoff: 0s → 1s → 4s.
    for delay_s in [0u64, 1, 4] {
        if delay_s > 0 {
            std::thread::sleep(Duration::from_secs(delay_s));
        }
        match agent
            .get(url)
            .set("User-Agent", &user_agent)
            .set("Accept", "application/json")
            .call()
        {
            Ok(resp) => {
                if resp.status() != 200 {
                    last_err = format!("HTTP {} from {url}", resp.status());
                    continue;
                }
                return resp
                    .into_json::<Value>()
                    .map_err(|e| UpgradeError::Other(format!("parse JSON: {e}")));
            }
            Err(e) => last_err = format!("ureq: {e}"),
        }
    }
    Err(UpgradeError::Other(format!("fetch {url}: {last_err}")))
}

fn http_get_bytes(url: &str, current_version: &str) -> Result<Vec<u8>, UpgradeError> {
    let agent = build_agent();
    let user_agent = format!("fluxmirror/{current_version}");
    let mut last_err = String::new();
    for delay_s in [0u64, 1, 4] {
        if delay_s > 0 {
            std::thread::sleep(Duration::from_secs(delay_s));
        }
        match agent.get(url).set("User-Agent", &user_agent).call() {
            Ok(resp) => {
                if resp.status() != 200 {
                    last_err = format!("HTTP {} from {url}", resp.status());
                    continue;
                }
                let mut buf = Vec::new();
                if let Err(e) = resp.into_reader().read_to_end(&mut buf) {
                    last_err = format!("read: {e}");
                    continue;
                }
                return Ok(buf);
            }
            Err(e) => last_err = format!("ureq: {e}"),
        }
    }
    Err(UpgradeError::Other(format!("download {url}: {last_err}")))
}

fn http_get_text(url: &str, current_version: &str) -> Result<String, UpgradeError> {
    let bytes = http_get_bytes(url, current_version)?;
    String::from_utf8(bytes).map_err(|e| UpgradeError::Other(format!("non-utf8 body: {e}")))
}

fn find_asset_url(assets: &[Value], name: &str) -> Option<String> {
    for a in assets {
        if a.get("name").and_then(|v| v.as_str()) == Some(name) {
            if let Some(url) = a.get("browser_download_url").and_then(|v| v.as_str()) {
                return Some(url.to_string());
            }
        }
    }
    None
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

/// Parse the first whitespace-delimited token of the first line of a
/// `.sha256` file. Handles both `shasum -a 256` output (`<hex>  <name>`)
/// and bare hex digests. Returned hex is lowercased.
fn parse_sha256_line(s: &str) -> String {
    s.lines()
        .next()
        .unwrap_or("")
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_lowercase()
}

fn strip_v_prefix(s: &str) -> &str {
    s.strip_prefix('v').unwrap_or(s)
}

/// Parse the `MAJOR.MINOR.PATCH` core of a semver string. Strips a
/// leading `v` and any `-prerelease` / `+build` suffix. Returns None
/// on anything that is not three dot-separated u64s.
fn parse_semver(s: &str) -> Option<(u64, u64, u64)> {
    let stripped = strip_v_prefix(s.trim());
    let core = stripped
        .split(|c| c == '-' || c == '+')
        .next()
        .unwrap_or("");
    let mut parts = core.split('.');
    let major: u64 = parts.next()?.parse().ok()?;
    let minor: u64 = parts.next()?.parse().ok()?;
    let patch: u64 = parts.next()?.parse().ok()?;
    Some((major, minor, patch))
}

fn is_newer_semver(latest: &str, current: &str) -> bool {
    match (parse_semver(latest), parse_semver(current)) {
        (Some(a), Some(b)) => a > b,
        // Unparseable on either side: don't block, let the user retry
        // through the normal download / verify path.
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semver_compare() {
        assert!(is_newer_semver("v0.6.0", "0.5.7"));
        assert!(is_newer_semver("v1.0.0", "0.99.99"));
        assert!(!is_newer_semver("v0.5.7", "0.5.7"));
        assert!(!is_newer_semver("v0.5.6", "0.5.7"));
    }

    #[test]
    fn parse_sha_handles_shasum_format() {
        let line = "abcdef0123456789  fluxmirror-darwin-arm64\n";
        assert_eq!(parse_sha256_line(line), "abcdef0123456789");
    }

    #[test]
    fn parse_sha_handles_bare_hex() {
        let line = "ABCDEF0123456789\n";
        assert_eq!(parse_sha256_line(line), "abcdef0123456789");
    }

    #[test]
    fn sha256_known_vector() {
        // RFC test vector: sha256("abc")
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn current_target_is_known() {
        let t = current_target_triple();
        assert!(
            asset_suffix_for_target(t).is_some(),
            "asset_suffix unmapped for current target triple {t}"
        );
    }
}
