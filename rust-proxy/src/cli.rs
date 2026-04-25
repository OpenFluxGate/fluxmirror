use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "Usage: fluxmirror-proxy --server-name <name> --db <path> \
                     [--capture-c2s <path>] [--capture-s2c <path>] \
                     -- <server command...>";

#[derive(Debug)]
pub struct Cli {
    pub server_name: String,
    pub db_path: PathBuf,
    pub capture_c2s: Option<PathBuf>,
    pub capture_s2c: Option<PathBuf>,
    pub server_command: Vec<String>,
}

pub enum CliResult {
    Ok(Cli),
    HelpExit,
    Err(String),
}

pub fn parse(args: Vec<String>) -> CliResult {
    let mut iter = args.into_iter().skip(1).peekable();

    if let Some(first) = iter.peek() {
        if first == "--help" || first == "-h" {
            println!("{USAGE}");
            return CliResult::HelpExit;
        }
    }

    let mut server_name: Option<String> = None;
    let mut db_path: Option<PathBuf> = None;
    let mut capture_c2s: Option<PathBuf> = None;
    let mut capture_s2c: Option<PathBuf> = None;
    let mut server_command: Vec<String> = Vec::new();

    while let Some(a) = iter.next() {
        match a.as_str() {
            "--server-name" => match iter.next() {
                Some(v) => server_name = Some(v),
                None => return CliResult::Err("--server-name requires a value".into()),
            },
            "--db" => match iter.next() {
                Some(v) => db_path = Some(PathBuf::from(v)),
                None => return CliResult::Err("--db requires a value".into()),
            },
            "--capture-c2s" => match iter.next() {
                Some(v) => capture_c2s = Some(PathBuf::from(v)),
                None => return CliResult::Err("--capture-c2s requires a value".into()),
            },
            "--capture-s2c" => match iter.next() {
                Some(v) => capture_s2c = Some(PathBuf::from(v)),
                None => return CliResult::Err("--capture-s2c requires a value".into()),
            },
            "--" => {
                server_command.extend(iter.by_ref());
            }
            other => return CliResult::Err(format!("Unknown option: {other}\n{USAGE}")),
        }
    }

    let server_name = match server_name {
        Some(s) => s,
        None => return CliResult::Err(format!("--server-name is required\n{USAGE}")),
    };
    let db_path = match db_path {
        Some(p) => p,
        None => return CliResult::Err(format!("--db is required\n{USAGE}")),
    };
    if server_command.is_empty() {
        return CliResult::Err(format!("Server command is required after --\n{USAGE}"));
    }

    CliResult::Ok(Cli {
        server_name,
        db_path,
        capture_c2s,
        capture_s2c,
        server_command,
    })
}

#[allow(dead_code)]
pub fn exit_with_usage_error(msg: &str) -> ExitCode {
    eprintln!("{msg}");
    ExitCode::from(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &[&str]) -> Vec<String> {
        std::iter::once("fluxmirror-proxy".to_string())
            .chain(s.iter().map(|s| s.to_string()))
            .collect()
    }

    #[test]
    fn parses_minimal() {
        let r = parse(args(&["--server-name", "fs", "--db", "/tmp/x.db", "--", "echo", "hi"]));
        match r {
            CliResult::Ok(c) => {
                assert_eq!(c.server_name, "fs");
                assert_eq!(c.db_path, PathBuf::from("/tmp/x.db"));
                assert_eq!(c.server_command, vec!["echo", "hi"]);
                assert!(c.capture_c2s.is_none());
            }
            _ => panic!("expected Ok"),
        }
    }

    #[test]
    fn parses_captures() {
        let r = parse(args(&[
            "--server-name", "fs", "--db", "/tmp/x.db",
            "--capture-c2s", "/tmp/c2s",
            "--capture-s2c", "/tmp/s2c",
            "--", "cat",
        ]));
        match r {
            CliResult::Ok(c) => {
                assert_eq!(c.capture_c2s, Some(PathBuf::from("/tmp/c2s")));
                assert_eq!(c.capture_s2c, Some(PathBuf::from("/tmp/s2c")));
            }
            _ => panic!("expected Ok"),
        }
    }

    #[test]
    fn missing_server_name_errs() {
        let r = parse(args(&["--db", "/tmp/x.db", "--", "cat"]));
        assert!(matches!(r, CliResult::Err(_)));
    }

    #[test]
    fn missing_command_errs() {
        let r = parse(args(&["--server-name", "fs", "--db", "/tmp/x.db"]));
        assert!(matches!(r, CliResult::Err(_)));
    }

    #[test]
    fn help_returns_help_exit() {
        let r = parse(args(&["--help"]));
        assert!(matches!(r, CliResult::HelpExit));
    }
}
