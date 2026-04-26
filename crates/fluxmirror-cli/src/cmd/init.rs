// fluxmirror init — first-run wizard.
//
// STEP 8 will implement: language + timezone selection, directory
// creation, optional `--non-interactive`, optional `--advanced` flow.

use std::process::ExitCode;

pub fn run(
    _advanced: bool,
    _non_interactive: bool,
    _language: Option<String>,
    _timezone: Option<String>,
) -> ExitCode {
    eprintln!("fluxmirror init: not yet implemented (STEP 8)");
    ExitCode::from(2)
}
