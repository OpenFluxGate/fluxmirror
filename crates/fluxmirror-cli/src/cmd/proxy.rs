// fluxmirror proxy — long-running stdio MCP relay entry.
//
// Thin runtime wrapper around the fluxmirror-proxy lib. Parses CLI
// args, opens the SQLite event store, spawns the writer thread,
// spawns the child MCP server, runs the bidirectional bridge until
// EOF, then tears down the child + writer cleanly.
//
// Usage:
//   fluxmirror proxy --server-name <name> --db <path> \
//     [--capture-c2s <path>] [--capture-s2c <path>] \
//     -- <real MCP server command and args...>

use std::fs::OpenOptions;
use std::process::ExitCode;
use std::sync::mpsc;

use fluxmirror_proxy::{bridge, child, cli, store, writer};

pub fn run(argv: Vec<String>) -> ExitCode {
    // The lib's CLI parser was authored for a binary entry point and so
    // skips the program name (`args[0]`). We call it from a subcommand
    // dispatcher where argv already excludes the program name; prepend
    // a placeholder so the existing skip(1) lands on the right element.
    let mut full = Vec::with_capacity(argv.len() + 1);
    full.push("fluxmirror".to_string());
    full.extend(argv);

    let parsed = match cli::parse(full) {
        cli::CliResult::Ok(c) => c,
        cli::CliResult::HelpExit => return ExitCode::SUCCESS,
        cli::CliResult::Err(msg) => {
            eprintln!("{msg}");
            return ExitCode::from(2);
        }
    };

    let store = match store::EventStore::open(&parsed.db_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[fluxmirror-proxy] FATAL open db: {e}");
            return ExitCode::from(1);
        }
    };

    let (tx, rx) = mpsc::channel();
    let writer_handle = writer::spawn(store, rx);

    // Spawn child MCP server.
    let mut child_proc = match child::ChildProc::spawn(&parsed.server_command) {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "[fluxmirror-proxy] FATAL spawn server {:?}: {e}",
                parsed.server_command
            );
            // Drop tx so writer can drain and exit cleanly.
            drop(tx);
            let _ = writer_handle.thread.join();
            return ExitCode::from(1);
        }
    };
    eprintln!(
        "[fluxmirror-proxy] spawned pid={} server-name={}",
        child_proc.pid(),
        parsed.server_name
    );

    let child_in = child_proc
        .take_stdin()
        .expect("child stdin not piped (impossible)");
    let child_out = child_proc
        .take_stdout()
        .expect("child stdout not piped (impossible)");

    let capture_c2s = open_append(parsed.capture_c2s.as_deref());
    if capture_c2s.is_some() {
        eprintln!(
            "[fluxmirror-proxy] capturing c2s to {:?}",
            parsed.capture_c2s.as_ref().unwrap()
        );
    }
    let capture_s2c = open_append(parsed.capture_s2c.as_deref());
    if capture_s2c.is_some() {
        eprintln!(
            "[fluxmirror-proxy] capturing s2c to {:?}",
            parsed.capture_s2c.as_ref().unwrap()
        );
    }

    let parent_in = std::io::stdin();
    let parent_out = std::io::stdout();

    let (c2s_join, s2c_join) = bridge::run(
        parent_in,
        parent_out,
        child_in,
        child_out,
        capture_c2s,
        capture_s2c,
        tx,
        parsed.server_name,
    );

    // Wait for both relays.
    let _ = c2s_join.join();
    let _ = s2c_join.join();

    // Tear down child cleanly.
    child_proc.shutdown();

    // Writer's last sender (the original tx) was moved into bridge::run
    // and got dropped when the relays exited; the writer will see
    // Disconnected, drain remaining events, and exit. We block here so
    // buffered events are flushed before main returns.
    if let Err(e) = writer_handle.thread.join() {
        eprintln!("[fluxmirror-proxy] WARN writer thread panicked: {e:?}");
    }

    ExitCode::SUCCESS
}

fn open_append(path: Option<&std::path::Path>) -> Option<std::fs::File> {
    let p = path?;
    match OpenOptions::new().create(true).append(true).open(p) {
        Ok(f) => Some(f),
        Err(e) => {
            eprintln!("[fluxmirror-proxy] WARN cannot open capture {p:?}: {e}");
            None
        }
    }
}
