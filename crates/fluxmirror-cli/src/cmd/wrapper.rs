// fluxmirror wrapper — wrapper engine selection.
//
// STEP 7 will implement: probe the host shell environment, pick a wrapper
// kind (bash | powershell | etc.), persist the choice, and emit the
// matching shim under <plugin>/hooks/.

use clap::Subcommand;
use std::process::ExitCode;

#[derive(Subcommand)]
pub enum WrapperOp {
    /// Print the currently selected wrapper kind.
    Show,
    /// Probe the host environment and recommend a wrapper kind.
    Probe,
    /// Force a specific wrapper kind.
    Set { kind: String },
}

pub fn run(op: WrapperOp) -> ExitCode {
    let _ = op;
    eprintln!("fluxmirror wrapper: not yet implemented (STEP 7)");
    ExitCode::from(2)
}
