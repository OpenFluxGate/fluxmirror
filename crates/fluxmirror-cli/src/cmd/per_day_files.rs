// fluxmirror per-day-files — per-day new vs edited file counts.
//
// STEP 5 will implement: from agent_events with file-typed tools, emit
// one row per local day with new/edited counts.

use std::path::PathBuf;
use std::process::ExitCode;

pub fn run(_db: PathBuf, _tz: String, _start: String, _end: String) -> ExitCode {
    eprintln!("fluxmirror per-day-files: not yet implemented (STEP 5)");
    ExitCode::from(2)
}
