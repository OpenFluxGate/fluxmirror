// fluxmirror config — read/write/inspect config layers.
//
// STEP 8 will implement: get/set/show/explain over the layered config
// (defaults < user < project < env).

use clap::Subcommand;
use std::process::ExitCode;

#[derive(Subcommand)]
pub enum ConfigOp {
    /// Print the resolved value of a single key.
    Get { key: String },
    /// Set a key in the user config layer.
    Set { key: String, value: String },
    /// Print the fully-resolved config (all layers merged).
    Show,
    /// Print each key with the layer that won.
    Explain,
}

pub fn run(op: ConfigOp) -> ExitCode {
    let _ = op;
    eprintln!("fluxmirror config: not yet implemented (STEP 8)");
    ExitCode::from(2)
}
