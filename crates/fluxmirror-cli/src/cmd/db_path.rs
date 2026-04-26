// fluxmirror db-path — print the platform-default DB path.
//
// Trivial enough to ship in STEP 4: slash commands need a stable way to
// discover the active database path without re-implementing the
// platform-specific resolution rules.

use fluxmirror_core::paths;
use std::process::ExitCode;

pub fn run() -> ExitCode {
    println!("{}", paths::default_db_path().display());
    ExitCode::SUCCESS
}
