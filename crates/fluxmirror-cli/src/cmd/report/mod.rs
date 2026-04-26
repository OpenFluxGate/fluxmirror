// fluxmirror reports — binary subcommands that emit a finished
// human-readable report on stdout.
//
// Each submodule owns one slash command (`/fluxmirror:<name>`) and
// exposes a `run(args) -> ExitCode` entry point. Phase 2 milestone M1
// migrates these one at a time; modules that haven't been ported yet
// print a "not yet implemented" line and exit 2 so callers can detect
// the gap without crashing.
//
// All modules share the `ReportFormat` enum so every subcommand carries
// the same `--format` surface even before json / markdown ship.

use std::fmt;

pub mod about;
pub mod agent;
pub mod agents;
pub mod compare;
pub(crate) mod day;
pub mod html;
pub mod today;
pub(crate) mod tools;
pub mod week;
pub mod yesterday;

/// Output format selected by the caller via `--format`.
///
/// `Human` is the fully-implemented default. `Html` is implemented for
/// the `week` subcommand only (M5 option A — the weekly digest card);
/// every other report still returns a "not yet implemented" line for
/// `Html`. `Json` and `Markdown` remain reserved on the surface across
/// all reports so client tooling can rely on the flag existing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum ReportFormat {
    Human,
    Json,
    Markdown,
    Html,
}

impl Default for ReportFormat {
    fn default() -> Self {
        ReportFormat::Human
    }
}

impl fmt::Display for ReportFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReportFormat::Human => f.write_str("human"),
            ReportFormat::Json => f.write_str("json"),
            ReportFormat::Markdown => f.write_str("markdown"),
            ReportFormat::Html => f.write_str("html"),
        }
    }
}
