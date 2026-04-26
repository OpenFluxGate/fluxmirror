// fluxmirror window — compute a TZ-aware time range.
//
// STEP 5 will implement: today | yesterday | week, returning ISO 8601
// start/end timestamps in the requested timezone.

use std::process::ExitCode;

pub fn run(_tz: String, _period: String) -> ExitCode {
    eprintln!("fluxmirror window: not yet implemented (STEP 5)");
    ExitCode::from(2)
}
