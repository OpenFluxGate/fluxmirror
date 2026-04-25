// fluxmirror-hook — single-binary tool-call hook.
//
// Reads a tool-call JSON payload on stdin, writes one JSONL line to
// ~/<agent>/session-logs/YYYY-MM-DD.jsonl and one parameter-bound row
// to the FluxMirror SQLite database.
//
// Designed to never break the calling agent: every IO error is logged
// to ~/.fluxmirror/hook-errors.log (with 5 MiB rotation) and swallowed.
// Process exit code is always 0.
//
// CLI:
//   fluxmirror-hook                   # auto-detect kind from env
//   fluxmirror-hook --kind claude     # claude or qwen (resolved via env)
//   fluxmirror-hook --kind gemini     # always gemini-cli
//
// Env vars:
//   FLUXMIRROR_DB         override DB path
//   FLUXMIRROR_SKIP_SELF  if "1" + FLUXMIRROR_SELF_REPO set, skip self-noise
//   FLUXMIRROR_SELF_REPO  absolute path to fluxmirror repo
//   QWEN_CODE_NO_RELAUNCH set when running under Qwen Code
//   QWEN_PROJECT_DIR      set when running under Qwen Code

use rusqlite::{params, Connection};
use serde_json::Value;
use std::env;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

const ERR_LOG_MAX_BYTES: u64 = 5 * 1024 * 1024;

fn main() -> ExitCode {
    // Always exit 0 — never break the calling agent over telemetry failure.
    let _ = run();
    ExitCode::SUCCESS
}

#[derive(Debug, Clone, Copy)]
enum Kind {
    Claude,
    Gemini,
}

fn run() -> Result<(), String> {
    let kind = parse_kind(env::args().collect());

    // Read stdin
    let mut input = String::new();
    io::stdin()
        .read_to_string(&mut input)
        .map_err(|e| format!("read stdin: {e}"))?;

    // Parse JSON
    let v: Value = serde_json::from_str(&input).map_err(|e| {
        log_error(&format!("invalid JSON ({e}); first 120 bytes: {:?}", input.chars().take(120).collect::<String>()));
        format!("parse: {e}")
    })?;

    let tool = match v.get("tool_name").and_then(|s| s.as_str()) {
        Some(t) if !t.is_empty() => t.to_string(),
        _ => return Ok(()), // missing tool_name → silently no-op (matches bash behavior)
    };
    let session = v.get("session_id").and_then(|s| s.as_str()).unwrap_or("unknown").to_string();
    let cwd = v.get("cwd").and_then(|s| s.as_str()).unwrap_or("unknown").to_string();
    let detail = extract_detail(&tool, v.get("tool_input"));

    // Resolve agent label + log base
    let (agent, log_base) = resolve_agent(kind);

    // Self-noise filter
    if should_skip_self_noise(&tool, &cwd, &detail) {
        return Ok(());
    }

    // Timestamp (UTC, ISO 8601 second precision)
    let ts = format_utc_iso8601(SystemTime::now());

    // JSONL append (best-effort)
    if let Err(e) = append_jsonl(&log_base, &ts, &session, &tool, &detail, &cwd) {
        log_error(&format!("jsonl append: {e}"));
    }

    // SQLite write (best-effort, with full error logging)
    let db_path = resolve_db_path();
    if let Err(e) = sqlite_write(&db_path, &ts, &agent, &session, &tool, &detail, &cwd, &input) {
        log_error(&format!("sqlite write (agent={agent} tool={tool}): {e}"));
    }

    Ok(())
}

fn parse_kind(args: Vec<String>) -> Kind {
    // very small CLI: look for --kind <value>
    let mut iter = args.into_iter().skip(1);
    while let Some(a) = iter.next() {
        if a == "--kind" {
            if let Some(v) = iter.next() {
                match v.as_str() {
                    "gemini" => return Kind::Gemini,
                    _ => return Kind::Claude,
                }
            }
        }
    }
    Kind::Claude
}

fn resolve_agent(kind: Kind) -> (String, PathBuf) {
    let home = home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
    match kind {
        Kind::Gemini => ("gemini-cli".to_string(), home.join(".gemini")),
        Kind::Claude => {
            // Qwen reuses the Claude plugin. Detect via env signals.
            if env::var_os("QWEN_CODE_NO_RELAUNCH").is_some()
                || env::var_os("QWEN_PROJECT_DIR").is_some()
            {
                ("qwen-code".to_string(), home.join(".qwen"))
            } else {
                ("claude-code".to_string(), home.join(".claude"))
            }
        }
    }
}

fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME").map(PathBuf::from)
}

fn resolve_db_path() -> PathBuf {
    if let Some(p) = env::var_os("FLUXMIRROR_DB") {
        return PathBuf::from(p);
    }
    let home = home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
    home.join("Library/Application Support/fluxmirror/events.db")
}

fn extract_detail(tool: &str, input: Option<&Value>) -> String {
    let raw = match (tool, input) {
        // shell
        ("Bash", Some(o)) => first_string(o, &["command"]),
        ("run_shell_command", Some(o)) => first_string(o, &["command", "cmd"]),
        ("BashOutput", Some(o)) | ("KillBash", Some(o)) | ("kill_shell", Some(o)) => {
            first_string(o, &["bash_id", "shell_id"])
        }
        // file IO
        ("Read", Some(o)) | ("Write", Some(o)) | ("Edit", Some(o))
        | ("MultiEdit", Some(o)) | ("NotebookEdit", Some(o)) => {
            first_string(o, &["file_path", "notebook_path"])
        }
        ("read_file", Some(o)) | ("read_many_files", Some(o)) | ("write_file", Some(o))
        | ("edit_file", Some(o)) | ("replace", Some(o)) => {
            first_string(o, &["absolute_path", "path", "file_path"])
        }
        // search / glob
        ("Grep", Some(o)) | ("search_file_content", Some(o)) => {
            first_string(o, &["pattern", "query"])
        }
        ("Glob", Some(o)) | ("glob", Some(o)) => first_string(o, &["pattern"]),
        // web
        ("WebFetch", Some(o)) | ("web_fetch", Some(o)) => first_string(o, &["url"]),
        ("WebSearch", Some(o)) | ("web_search", Some(o)) | ("google_web_search", Some(o)) => {
            first_string(o, &["query"])
        }
        // task / planning / memory
        ("Task", Some(o)) => first_string(o, &["description", "prompt"]),
        ("TodoWrite", Some(o)) | ("todo_write", Some(o)) => {
            if let Some(arr) = o.get("todos").and_then(|t| t.as_array()) {
                format!("[{} todos]", arr.len())
            } else {
                String::new()
            }
        }
        ("ExitPlanMode", Some(o)) => first_string(o, &["plan"]),
        ("save_memory", Some(o)) => first_string(o, &["fact", "content"]),
        // fallback: first scalar string in tool_input
        (_, Some(o)) => first_scalar_string(o),
        _ => String::new(),
    };
    // truncate to 200 bytes (matches bash `head -c 200` semantics)
    truncate_bytes(&raw, 200)
}

fn first_string(obj: &Value, keys: &[&str]) -> String {
    for k in keys {
        if let Some(s) = obj.get(*k).and_then(|v| v.as_str()) {
            if !s.is_empty() {
                return s.to_string();
            }
        }
    }
    String::new()
}

fn first_scalar_string(obj: &Value) -> String {
    if let Some(map) = obj.as_object() {
        for (_, v) in map {
            if let Some(s) = v.as_str() {
                if !s.is_empty() {
                    return s.to_string();
                }
            }
        }
    }
    String::new()
}

fn truncate_bytes(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    // truncate on a UTF-8 boundary
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

fn should_skip_self_noise(tool: &str, cwd: &str, detail: &str) -> bool {
    if env::var("FLUXMIRROR_SKIP_SELF").as_deref() != Ok("1") {
        return false;
    }
    let repo = match env::var("FLUXMIRROR_SELF_REPO") {
        Ok(v) if !v.is_empty() => v,
        _ => return false,
    };

    let is_shell = matches!(tool, "Bash" | "run_shell_command");
    if !is_shell {
        return false;
    }

    let cwd_real = canonical(cwd);
    let repo_real = canonical(&repo);
    if cwd_real.is_none() || repo_real.is_none() {
        return false;
    }
    let mut cwd_with_sep = cwd_real.unwrap().to_string_lossy().into_owned();
    cwd_with_sep.push('/');
    let mut repo_with_sep = repo_real.unwrap().to_string_lossy().into_owned();
    repo_with_sep.push('/');

    if !cwd_with_sep.starts_with(&repo_with_sep) {
        return false;
    }

    // detail looks like a fluxmirror DB query
    let lc = detail.to_ascii_lowercase();
    lc.contains("sqlite3") && lc.contains("events.db")
        || lc.contains("fluxmirror") && lc.contains(".db")
}

fn canonical<P: AsRef<Path>>(p: P) -> Option<PathBuf> {
    fs::canonicalize(p).ok()
}

fn append_jsonl(
    log_base: &Path,
    ts: &str,
    session: &str,
    tool: &str,
    detail: &str,
    cwd: &str,
) -> io::Result<()> {
    let dir = log_base.join("session-logs");
    fs::create_dir_all(&dir)?;
    let date = utc_date_yyyy_mm_dd(SystemTime::now());
    let file = dir.join(format!("{date}.jsonl"));

    let line = serde_json::json!({
        "ts": ts,
        "session": session,
        "tool": tool,
        "detail": detail,
        "cwd": cwd,
    });
    let mut f = OpenOptions::new().create(true).append(true).open(&file)?;
    writeln!(f, "{}", line)?;
    Ok(())
}

fn sqlite_write(
    db_path: &Path,
    ts: &str,
    agent: &str,
    session: &str,
    tool: &str,
    detail: &str,
    cwd: &str,
    raw_json: &str,
) -> Result<(), String> {
    if let Some(parent) = db_path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("mkdir {parent:?}: {e}"))?;
    }
    let conn = Connection::open(db_path).map_err(|e| format!("open: {e}"))?;
    conn.busy_timeout(std::time::Duration::from_secs(5))
        .map_err(|e| format!("busy_timeout: {e}"))?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS agent_events (
           id INTEGER PRIMARY KEY AUTOINCREMENT,
           ts TEXT NOT NULL,
           agent TEXT NOT NULL,
           session TEXT,
           tool TEXT,
           detail TEXT,
           cwd TEXT,
           raw_json TEXT
         );",
    )
    .map_err(|e| format!("schema: {e}"))?;
    conn.execute(
        "INSERT INTO agent_events (ts, agent, session, tool, detail, cwd, raw_json) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![ts, agent, session, tool, detail, cwd, raw_json],
    )
    .map_err(|e| format!("insert: {e}"))?;
    Ok(())
}

fn log_error(msg: &str) {
    let Some(home) = home_dir() else { return };
    let dir = home.join(".fluxmirror");
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    let log = dir.join("hook-errors.log");
    let _ = rotate_if_needed(&log);
    let ts = format_utc_iso8601(SystemTime::now());
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&log) {
        let _ = writeln!(f, "[{ts}] {msg}");
    }
}

fn rotate_if_needed(log: &Path) -> io::Result<()> {
    let meta = match fs::metadata(log) {
        Ok(m) => m,
        Err(_) => return Ok(()), // doesn't exist yet
    };
    if meta.len() < ERR_LOG_MAX_BYTES {
        return Ok(());
    }
    let backup = log.with_extension("log.1");
    fs::rename(log, &backup)
}

// Minimal time formatting — avoids pulling in chrono.
fn format_utc_iso8601(t: SystemTime) -> String {
    let secs = t
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let (y, mo, d, h, mi, s) = secs_to_ymdhms(secs);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

fn utc_date_yyyy_mm_dd(t: SystemTime) -> String {
    let secs = t
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let (y, mo, d, _, _, _) = secs_to_ymdhms(secs);
    format!("{y:04}-{mo:02}-{d:02}")
}

// Convert epoch seconds → (year, month, day, hour, minute, second) UTC.
// Handles dates from 1970 through far future. Algorithm: civil_from_days.
fn secs_to_ymdhms(secs: i64) -> (i32, u32, u32, u32, u32, u32) {
    let days = secs.div_euclid(86_400);
    let time = secs.rem_euclid(86_400) as u32;
    let h = time / 3_600;
    let mi = (time % 3_600) / 60;
    let s = time % 60;
    let (y, mo, d) = civil_from_days(days);
    (y, mo, d, h, mi, s)
}

// Howard Hinnant's date algorithm.
fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detail_bash_grabs_command_not_description() {
        let v: Value = serde_json::from_str(
            r#"{"description":"Listing","command":"ls -la"}"#,
        ).unwrap();
        assert_eq!(extract_detail("Bash", Some(&v)), "ls -la");
    }

    #[test]
    fn detail_run_shell_command_grabs_command() {
        let v: Value = serde_json::from_str(r#"{"description":"Print hi","command":"echo hi"}"#).unwrap();
        assert_eq!(extract_detail("run_shell_command", Some(&v)), "echo hi");
    }

    #[test]
    fn detail_read_grabs_file_path() {
        let v: Value = serde_json::from_str(r#"{"file_path":"/etc/hosts"}"#).unwrap();
        assert_eq!(extract_detail("Read", Some(&v)), "/etc/hosts");
    }

    #[test]
    fn detail_read_file_grabs_absolute_path() {
        let v: Value = serde_json::from_str(r#"{"absolute_path":"/etc/hosts"}"#).unwrap();
        assert_eq!(extract_detail("read_file", Some(&v)), "/etc/hosts");
    }

    #[test]
    fn detail_glob_grabs_pattern() {
        let v: Value = serde_json::from_str(r#"{"pattern":"**/*.md"}"#).unwrap();
        assert_eq!(extract_detail("Glob", Some(&v)), "**/*.md");
    }

    #[test]
    fn detail_webfetch_grabs_url() {
        let v: Value = serde_json::from_str(r#"{"url":"https://x","prompt":"y"}"#).unwrap();
        assert_eq!(extract_detail("WebFetch", Some(&v)), "https://x");
    }

    #[test]
    fn detail_websearch_grabs_query() {
        let v: Value = serde_json::from_str(r#"{"query":"hello world"}"#).unwrap();
        assert_eq!(extract_detail("WebSearch", Some(&v)), "hello world");
    }

    #[test]
    fn detail_todowrite_counts() {
        let v: Value = serde_json::from_str(r#"{"todos":[{"a":1},{"a":2},{"a":3}]}"#).unwrap();
        assert_eq!(extract_detail("TodoWrite", Some(&v)), "[3 todos]");
    }

    #[test]
    fn detail_unknown_tool_falls_back_to_first_string() {
        let v: Value = serde_json::from_str(r#"{"foo":"bar","num":42}"#).unwrap();
        assert_eq!(extract_detail("BrandNewTool", Some(&v)), "bar");
    }

    #[test]
    fn detail_truncates_to_200_bytes() {
        let big = "a".repeat(500);
        let v: Value = serde_json::from_str(&format!(r#"{{"command":"{big}"}}"#)).unwrap();
        let d = extract_detail("Bash", Some(&v));
        assert_eq!(d.len(), 200);
    }

    #[test]
    fn iso8601_known_epoch() {
        // 1_777_676_400 s after epoch = 2026-05-01T23:00:00Z (verified externally).
        let s = format_utc_iso8601(UNIX_EPOCH + std::time::Duration::from_secs(1_777_676_400));
        assert_eq!(s, "2026-05-01T23:00:00Z");
    }

    #[test]
    fn iso8601_unix_epoch_zero() {
        let s = format_utc_iso8601(UNIX_EPOCH);
        assert_eq!(s, "1970-01-01T00:00:00Z");
    }

    #[test]
    fn date_2024_leap_day() {
        // 2024-02-29T00:00:00Z = 1709164800
        let s = format_utc_iso8601(UNIX_EPOCH + std::time::Duration::from_secs(1_709_164_800));
        assert_eq!(s, "2024-02-29T00:00:00Z");
    }

    #[test]
    fn date_2000_y2k_leap() {
        // 2000-02-29T12:34:56Z = 951_827_696
        let s = format_utc_iso8601(UNIX_EPOCH + std::time::Duration::from_secs(951_827_696));
        assert_eq!(s, "2000-02-29T12:34:56Z");
    }
}
