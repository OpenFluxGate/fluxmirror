// fluxmirror sqlite — run a SQL query against the events DB.
//
// STEP 5 will implement: open the DB read-only, execute the supplied
// SELECT, and print rows pipe-separated to stdout for slash command
// consumption.

use std::path::PathBuf;
use std::process::ExitCode;

pub fn run(_db: PathBuf, _sql: String) -> ExitCode {
    eprintln!("fluxmirror sqlite: not yet implemented (STEP 5)");
    ExitCode::from(2)
}
