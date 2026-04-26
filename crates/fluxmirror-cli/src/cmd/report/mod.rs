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
pub mod today;
pub(crate) mod tools;
pub mod week;
pub mod yesterday;

/// Output format selected by the caller via `--format`.
///
/// `Human` is the only fully-implemented format in M1. The other two
/// are accepted at the surface so client tooling can rely on the flag
/// existing; the agents subcommand prints a "not yet implemented" line
/// for them and exits 2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum ReportFormat {
    Human,
    Json,
    Markdown,
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
        }
    }
}
