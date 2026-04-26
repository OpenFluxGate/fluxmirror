// fluxmirror histogram — hourly bucket aggregation.
//
// STEP 5 will implement: group `agent_events` by local hour over the
// supplied [start,end] range, optionally filtered by agent.

use std::path::PathBuf;
use std::process::ExitCode;

pub fn run(
    _db: PathBuf,
    _tz: String,
    _start: String,
    _end: String,
    _agent: Option<String>,
) -> ExitCode {
    eprintln!("fluxmirror histogram: not yet implemented (STEP 5)");
    ExitCode::from(2)
}
