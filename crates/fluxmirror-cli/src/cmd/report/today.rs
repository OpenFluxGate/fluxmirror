// fluxmirror today — daily activity report.
//
// M1 stub. Filled in by a subsequent slice once the agents migration
// pattern is locked in. The surface (`run(args) -> ExitCode`) matches
// the other report modules so wiring stays uniform.

use std::process::ExitCode;

pub fn run() -> ExitCode {
    eprintln!("fluxmirror today: not yet implemented (M1)");
    ExitCode::from(2)
}
