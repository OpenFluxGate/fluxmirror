// Tool-name normalization + per-tool detail extraction.
//
// Each agent CLI uses a different naming convention:
//
//   Claude Code: PascalCase            (Bash, Read, Write, WebFetch, ...)
//   Gemini CLI : snake_case            (run_shell_command, read_file, ...)
//   Qwen Code  : Claude-compatible PascalCase
//
// `normalize` collapses both flavours into a single `ToolKind`. The
// matching `ToolClass` is the coarse bucket used by reporting (Shell,
// Read, Write, Search, Web, Task, Meta, Other). Both slots are stored
// on `agent_events` so future fine-grained splits don't require a
// migration.
//
// `extract_detail` returns the per-tool primary string (command for
// Bash, file path for Read, URL for WebFetch, query for Grep, etc.),
// truncated to 200 bytes on a UTF-8 boundary — the same byte budget
// the original bash + rust-hook implementations honoured.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolKind {
    Bash,
    BashOutput,
    KillBash,
    Read,
    Write,
    Edit,
    MultiEdit,
    NotebookEdit,
    ReadFile,
    ReadManyFiles,
    WriteFile,
    EditFile,
    Replace,
    Grep,
    SearchFileContent,
    Glob,
    WebFetch,
    WebSearch,
    GoogleWebSearch,
    Task,
    TodoWrite,
    ExitPlanMode,
    SaveMemory,
    Other(String),
}

impl ToolKind {
    pub fn as_str(&self) -> &str {
        match self {
            ToolKind::Bash => "Bash",
            ToolKind::BashOutput => "BashOutput",
            ToolKind::KillBash => "KillBash",
            ToolKind::Read => "Read",
            ToolKind::Write => "Write",
            ToolKind::Edit => "Edit",
            ToolKind::MultiEdit => "MultiEdit",
            ToolKind::NotebookEdit => "NotebookEdit",
            ToolKind::ReadFile => "ReadFile",
            ToolKind::ReadManyFiles => "ReadManyFiles",
            ToolKind::WriteFile => "WriteFile",
            ToolKind::EditFile => "EditFile",
            ToolKind::Replace => "Replace",
            ToolKind::Grep => "Grep",
            ToolKind::SearchFileContent => "SearchFileContent",
            ToolKind::Glob => "Glob",
            ToolKind::WebFetch => "WebFetch",
            ToolKind::WebSearch => "WebSearch",
            ToolKind::GoogleWebSearch => "GoogleWebSearch",
            ToolKind::Task => "Task",
            ToolKind::TodoWrite => "TodoWrite",
            ToolKind::ExitPlanMode => "ExitPlanMode",
            ToolKind::SaveMemory => "SaveMemory",
            ToolKind::Other(s) => s.as_str(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolClass {
    Shell,
    Read,
    Write,
    Search,
    Web,
    Task,
    Meta,
    Other,
}

impl ToolClass {
    pub fn as_str(&self) -> &'static str {
        match self {
            ToolClass::Shell => "Shell",
            ToolClass::Read => "Read",
            ToolClass::Write => "Write",
            ToolClass::Search => "Search",
            ToolClass::Web => "Web",
            ToolClass::Task => "Task",
            ToolClass::Meta => "Meta",
            ToolClass::Other => "Other",
        }
    }
}

/// Map a raw tool name (whatever flavour the CLI emitted) to its
/// canonical `ToolKind` plus broad `ToolClass` bucket.
pub fn normalize(tool_raw: &str) -> (ToolKind, ToolClass) {
    match tool_raw {
        // shell
        "Bash" => (ToolKind::Bash, ToolClass::Shell),
        "run_shell_command" => (ToolKind::Bash, ToolClass::Shell),
        "BashOutput" => (ToolKind::BashOutput, ToolClass::Meta),
        "KillBash" => (ToolKind::KillBash, ToolClass::Meta),
        "kill_shell" => (ToolKind::KillBash, ToolClass::Meta),

        // file IO — Claude flavour
        "Read" => (ToolKind::Read, ToolClass::Read),
        "Write" => (ToolKind::Write, ToolClass::Write),
        "Edit" => (ToolKind::Edit, ToolClass::Write),
        "MultiEdit" => (ToolKind::MultiEdit, ToolClass::Write),
        "NotebookEdit" => (ToolKind::NotebookEdit, ToolClass::Write),

        // file IO — Gemini flavour
        "read_file" => (ToolKind::ReadFile, ToolClass::Read),
        "read_many_files" => (ToolKind::ReadManyFiles, ToolClass::Read),
        "write_file" => (ToolKind::WriteFile, ToolClass::Write),
        "edit_file" => (ToolKind::EditFile, ToolClass::Write),
        "replace" => (ToolKind::Replace, ToolClass::Write),

        // search / glob
        "Grep" => (ToolKind::Grep, ToolClass::Search),
        "search_file_content" => (ToolKind::SearchFileContent, ToolClass::Search),
        "Glob" => (ToolKind::Glob, ToolClass::Search),
        "glob" => (ToolKind::Glob, ToolClass::Search),

        // web
        "WebFetch" => (ToolKind::WebFetch, ToolClass::Web),
        "web_fetch" => (ToolKind::WebFetch, ToolClass::Web),
        "WebSearch" => (ToolKind::WebSearch, ToolClass::Web),
        "web_search" => (ToolKind::WebSearch, ToolClass::Web),
        "google_web_search" => (ToolKind::GoogleWebSearch, ToolClass::Web),

        // task / planning / memory
        "Task" => (ToolKind::Task, ToolClass::Task),
        "TodoWrite" => (ToolKind::TodoWrite, ToolClass::Meta),
        "todo_write" => (ToolKind::TodoWrite, ToolClass::Meta),
        "ExitPlanMode" => (ToolKind::ExitPlanMode, ToolClass::Meta),
        "save_memory" => (ToolKind::SaveMemory, ToolClass::Meta),

        other => (ToolKind::Other(other.to_string()), ToolClass::Other),
    }
}

/// Per-tool primary detail field, truncated to 200 bytes on a UTF-8
/// boundary. Caller passes the canonical kind (post-normalize) and the
/// raw `tool_input` JSON object as parsed by serde.
pub fn extract_detail(kind: &ToolKind, input: Option<&Value>) -> String {
    let raw = match (kind, input) {
        // shell
        (ToolKind::Bash, Some(o)) => first_string(o, &["command"]),
        (ToolKind::BashOutput, Some(o))
        | (ToolKind::KillBash, Some(o)) => first_string(o, &["bash_id", "shell_id"]),

        // file IO — Claude flavour
        (ToolKind::Read, Some(o))
        | (ToolKind::Write, Some(o))
        | (ToolKind::Edit, Some(o))
        | (ToolKind::MultiEdit, Some(o))
        | (ToolKind::NotebookEdit, Some(o)) => first_string(o, &["file_path", "notebook_path"]),

        // file IO — Gemini flavour
        (ToolKind::ReadFile, Some(o))
        | (ToolKind::ReadManyFiles, Some(o))
        | (ToolKind::WriteFile, Some(o))
        | (ToolKind::EditFile, Some(o))
        | (ToolKind::Replace, Some(o)) => first_string(o, &["absolute_path", "path", "file_path"]),

        // search
        (ToolKind::Grep, Some(o)) | (ToolKind::SearchFileContent, Some(o)) => {
            first_string(o, &["pattern", "query"])
        }
        (ToolKind::Glob, Some(o)) => first_string(o, &["pattern"]),

        // web
        (ToolKind::WebFetch, Some(o)) => first_string(o, &["url"]),
        (ToolKind::WebSearch, Some(o)) | (ToolKind::GoogleWebSearch, Some(o)) => {
            first_string(o, &["query"])
        }

        // task / planning / memory
        (ToolKind::Task, Some(o)) => first_string(o, &["description", "prompt"]),
        (ToolKind::TodoWrite, Some(o)) => {
            if let Some(arr) = o.get("todos").and_then(|t| t.as_array()) {
                format!("[{} todos]", arr.len())
            } else {
                String::new()
            }
        }
        (ToolKind::ExitPlanMode, Some(o)) => first_string(o, &["plan"]),
        (ToolKind::SaveMemory, Some(o)) => first_string(o, &["fact", "content"]),

        // fallback: first scalar string in tool_input
        (_, Some(o)) => first_scalar_string(o),
        _ => String::new(),
    };
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
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn norm(s: &str) -> ToolKind {
        normalize(s).0
    }
    fn class(s: &str) -> ToolClass {
        normalize(s).1
    }

    // ---------- normalize() table ----------

    #[test]
    fn normalize_shell_variants() {
        assert_eq!(norm("Bash"), ToolKind::Bash);
        assert_eq!(class("Bash"), ToolClass::Shell);
        assert_eq!(norm("run_shell_command"), ToolKind::Bash);
        assert_eq!(class("run_shell_command"), ToolClass::Shell);
        assert_eq!(class("BashOutput"), ToolClass::Meta);
        assert_eq!(class("KillBash"), ToolClass::Meta);
        assert_eq!(class("kill_shell"), ToolClass::Meta);
    }

    #[test]
    fn normalize_read_variants() {
        assert_eq!(norm("Read"), ToolKind::Read);
        assert_eq!(class("Read"), ToolClass::Read);
        assert_eq!(norm("read_file"), ToolKind::ReadFile);
        assert_eq!(class("read_file"), ToolClass::Read);
        assert_eq!(class("read_many_files"), ToolClass::Read);
    }

    #[test]
    fn normalize_write_variants() {
        assert_eq!(class("Write"), ToolClass::Write);
        assert_eq!(class("Edit"), ToolClass::Write);
        assert_eq!(class("MultiEdit"), ToolClass::Write);
        assert_eq!(class("NotebookEdit"), ToolClass::Write);
        assert_eq!(class("write_file"), ToolClass::Write);
        assert_eq!(class("edit_file"), ToolClass::Write);
        assert_eq!(class("replace"), ToolClass::Write);
    }

    #[test]
    fn normalize_search_variants() {
        assert_eq!(class("Grep"), ToolClass::Search);
        assert_eq!(class("search_file_content"), ToolClass::Search);
        assert_eq!(class("Glob"), ToolClass::Search);
        assert_eq!(class("glob"), ToolClass::Search);
    }

    #[test]
    fn normalize_web_variants() {
        assert_eq!(class("WebFetch"), ToolClass::Web);
        assert_eq!(class("web_fetch"), ToolClass::Web);
        assert_eq!(class("WebSearch"), ToolClass::Web);
        assert_eq!(class("web_search"), ToolClass::Web);
        assert_eq!(class("google_web_search"), ToolClass::Web);
    }

    #[test]
    fn normalize_task_meta_variants() {
        assert_eq!(class("Task"), ToolClass::Task);
        assert_eq!(class("TodoWrite"), ToolClass::Meta);
        assert_eq!(class("todo_write"), ToolClass::Meta);
        assert_eq!(class("ExitPlanMode"), ToolClass::Meta);
        assert_eq!(class("save_memory"), ToolClass::Meta);
    }

    #[test]
    fn normalize_unknown_falls_through_to_other() {
        let (k, c) = normalize("BrandNewTool");
        assert!(matches!(k, ToolKind::Other(ref s) if s == "BrandNewTool"));
        assert_eq!(c, ToolClass::Other);
    }

    // ---------- extract_detail() per tool ----------

    fn detail(raw: &str, json: &str) -> String {
        let v: Value = serde_json::from_str(json).unwrap();
        let (kind, _) = normalize(raw);
        extract_detail(&kind, Some(&v))
    }

    #[test]
    fn detail_bash_grabs_command_not_description() {
        assert_eq!(
            detail("Bash", r#"{"description":"Listing","command":"ls -la"}"#),
            "ls -la"
        );
    }

    #[test]
    fn detail_run_shell_command_grabs_command() {
        assert_eq!(
            detail(
                "run_shell_command",
                r#"{"description":"Print hi","command":"echo hi"}"#
            ),
            "echo hi"
        );
    }

    #[test]
    fn detail_read_grabs_file_path() {
        assert_eq!(detail("Read", r#"{"file_path":"/etc/hosts"}"#), "/etc/hosts");
    }

    #[test]
    fn detail_read_file_grabs_absolute_path() {
        assert_eq!(
            detail("read_file", r#"{"absolute_path":"/etc/hosts"}"#),
            "/etc/hosts"
        );
    }

    #[test]
    fn detail_glob_grabs_pattern() {
        assert_eq!(detail("Glob", r#"{"pattern":"**/*.md"}"#), "**/*.md");
    }

    #[test]
    fn detail_webfetch_grabs_url() {
        assert_eq!(
            detail("WebFetch", r#"{"url":"https://x","prompt":"y"}"#),
            "https://x"
        );
    }

    #[test]
    fn detail_websearch_grabs_query() {
        assert_eq!(
            detail("WebSearch", r#"{"query":"hello world"}"#),
            "hello world"
        );
    }

    #[test]
    fn detail_todowrite_counts_array() {
        assert_eq!(
            detail("TodoWrite", r#"{"todos":[{"a":1},{"a":2},{"a":3}]}"#),
            "[3 todos]"
        );
    }

    #[test]
    fn detail_unknown_tool_falls_back_to_first_string() {
        assert_eq!(
            detail("BrandNewTool", r#"{"foo":"bar","num":42}"#),
            "bar"
        );
    }

    #[test]
    fn detail_truncates_to_200_bytes() {
        let big = "a".repeat(500);
        let json = format!(r#"{{"command":"{big}"}}"#);
        let d = detail("Bash", &json);
        assert_eq!(d.len(), 200);
    }

    #[test]
    fn detail_truncate_respects_utf8_boundary() {
        // 100 ✓ chars (each 3 bytes) = 300 bytes — overshoots 200.
        // truncate_bytes must back up to a char boundary.
        let s = "✓".repeat(100);
        let json = format!(r#"{{"command":"{s}"}}"#);
        let d = detail("Bash", &json);
        assert!(d.len() <= 200);
        // resulting string must still be valid UTF-8 (it is by construction
        // since &str slice on boundary), and divisible by 3 == only ✓ chars.
        assert_eq!(d.len() % 3, 0);
    }

    #[test]
    fn detail_no_input_yields_empty() {
        assert_eq!(extract_detail(&ToolKind::Bash, None), "");
    }

    #[test]
    fn detail_empty_string_skipped_in_first_string() {
        assert_eq!(
            detail("Bash", r#"{"command":"","description":"fallback"}"#),
            ""
        );
    }
}
