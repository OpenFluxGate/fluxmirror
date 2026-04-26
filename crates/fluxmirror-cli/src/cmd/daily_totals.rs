// fluxmirror daily-totals — per-day totals across a week range.
//
// STEP 5 will implement: count agent_events per local day for each agent,
// emit one row per (date, agent).

use std::path::PathBuf;
use std::process::ExitCode;

pub fn run(_db: PathBuf, _tz: String, _start: String, _end: String) -> ExitCode {
    eprintln!("fluxmirror daily-totals: not yet implemented (STEP 5)");
    ExitCode::from(2)
}
