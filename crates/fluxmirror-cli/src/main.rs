// fluxmirror — single binary entry point.
//
// STEP 1 dispatcher. Parses argv[1] as the subcommand name and forwards
// the rest to the matching cmd module. clap-based parsing arrives in
// STEP 4; for now we keep the surface tiny so subcommand behavior is a
// 1:1 lift of the previous fluxmirror-hook / fluxmirror-proxy binaries.

use std::env;
use std::process::ExitCode;

mod cmd;

fn main() -> ExitCode {
    let mut argv = env::args().skip(1);
    match argv.next().as_deref() {
        Some("hook") => cmd::hook::run(argv.collect()),
        Some("proxy") => cmd::proxy::run(argv.collect()),
        _ => {
            eprintln!("usage: fluxmirror <hook|proxy> [args...]");
            ExitCode::from(2)
        }
    }
}
