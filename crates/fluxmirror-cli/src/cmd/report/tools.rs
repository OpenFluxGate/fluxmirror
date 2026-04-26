// Tool-name classification constants shared across report subcommands.
//
// Each list groups the tool names every supported agent (Claude, Qwen,
// Gemini) emits for a given action class. We classify in Rust rather
// than SQL so the lists stay in one place and the SQL queries stay
// plain `SELECT`s without IN-list interpolation.

/// Tool names that count as "writes" for the share-of-calls breakdown.
/// Mirrors the legacy slash command surface (Edit, Write, MultiEdit,
/// plus the gemini/qwen camelCase variants).
pub(crate) const WRITE_TOOLS: &[&str] = &[
    "Edit",
    "Write",
    "MultiEdit",
    "edit_file",
    "write_file",
    "replace",
];

/// Tool names that count as "reads" — file inspection without mutation.
pub(crate) const READ_TOOLS: &[&str] = &["Read", "read_file", "read_many_files"];

/// Tool names that invoke a shell command.
pub(crate) const SHELL_TOOLS: &[&str] = &["Bash", "run_shell_command"];

/// Returns `true` if `tool` is in `WRITE_TOOLS`.
pub(crate) fn is_write(tool: &str) -> bool {
    WRITE_TOOLS.contains(&tool)
}

/// Returns `true` if `tool` is in `READ_TOOLS`.
pub(crate) fn is_read(tool: &str) -> bool {
    READ_TOOLS.contains(&tool)
}

/// Returns `true` if `tool` is in `SHELL_TOOLS`.
pub(crate) fn is_shell(tool: &str) -> bool {
    SHELL_TOOLS.contains(&tool)
}
