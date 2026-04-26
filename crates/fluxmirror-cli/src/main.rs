// fluxmirror — single binary entry point.
//
// STEP 4 dispatcher: clap-derive subcommand surface for the full Phase 1
// CLI. Most subcommands are stubs that print "not yet implemented (STEP N)"
// and exit 2 — they get filled in by later STEPs. The two existing
// subcommands (`hook` and `proxy`) keep their original `Vec<String>`
// signatures so their internal arg parsers remain authoritative.

use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;
use std::process::ExitCode;

mod cmd;

use cmd::config::ConfigOp;
use cmd::wrapper::WrapperOp;

#[derive(Parser)]
#[command(
    name = "fluxmirror",
    version,
    about = "Multi-agent observability for local AI tooling"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Receive a tool-call JSON payload on stdin and record it.
    Hook {
        /// Force a specific agent kind. Defaults to claude/qwen via env.
        #[arg(long, value_enum)]
        kind: Option<HookKind>,
    },

    /// Long-running stdio MCP relay. All trailing args (including the
    /// `--` and child command) are forwarded verbatim to the proxy.
    #[command(allow_hyphen_values = true, trailing_var_arg = true)]
    Proxy {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// First-run wizard.
    Init {
        #[arg(long)]
        advanced: bool,
        #[arg(long)]
        non_interactive: bool,
        #[arg(long)]
        language: Option<String>,
        #[arg(long)]
        timezone: Option<String>,
    },

    /// Read / write / inspect config layers.
    Config {
        #[command(subcommand)]
        op: ConfigOp,
    },

    /// Wrapper engine selection.
    Wrapper {
        #[command(subcommand)]
        op: WrapperOp,
    },

    /// Health check for db / agents / wrapper / version / perms.
    Doctor,

    /// Print platform-default DB path.
    DbPath,

    /// Compute a TZ-aware time range (today / yesterday / week).
    Window(WindowArgs),

    /// Hourly bucket aggregation.
    Histogram(HistogramArgs),

    /// Per-day totals across a week range.
    DailyTotals(DailyTotalsArgs),

    /// Per-day new vs edited file counts across a week range.
    PerDayFiles(PerDayFilesArgs),

    /// Run a SQL query against the events DB and print rows pipe-separated.
    Sqlite(SqliteArgs),
}

#[derive(Args)]
struct WindowArgs {
    #[arg(long)]
    tz: String,
    /// today | yesterday | week (validated downstream).
    #[arg(long)]
    period: String,
}

#[derive(Args)]
struct HistogramArgs {
    #[arg(long)]
    db: PathBuf,
    #[arg(long)]
    tz: String,
    #[arg(long)]
    start: String,
    #[arg(long)]
    end: String,
    #[arg(long)]
    agent: Option<String>,
}

#[derive(Args)]
struct DailyTotalsArgs {
    #[arg(long)]
    db: PathBuf,
    #[arg(long)]
    tz: String,
    #[arg(long)]
    start: String,
    #[arg(long)]
    end: String,
}

#[derive(Args)]
struct PerDayFilesArgs {
    #[arg(long)]
    db: PathBuf,
    #[arg(long)]
    tz: String,
    #[arg(long)]
    start: String,
    #[arg(long)]
    end: String,
}

#[derive(Args)]
struct SqliteArgs {
    #[arg(long)]
    db: PathBuf,
    /// SQL query (typically a SELECT). Quote it.
    sql: String,
}

#[derive(Copy, Clone, ValueEnum)]
enum HookKind {
    Claude,
    Gemini,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Hook { kind } => {
            // hook::run still does its own --kind walk (pre-clap contract).
            // Re-encode the typed enum as a single --kind <value> pair so
            // hook::parse_kind picks it up unchanged. None → empty Vec
            // → defaults to Claude inside hook::parse_kind.
            let mut argv: Vec<String> = Vec::new();
            if let Some(k) = kind {
                argv.push("--kind".into());
                argv.push(match k {
                    HookKind::Claude => "claude".into(),
                    HookKind::Gemini => "gemini".into(),
                });
            }
            cmd::hook::run(argv)
        }
        Cmd::Proxy { args } => cmd::proxy::run(args),
        Cmd::Init {
            advanced,
            non_interactive,
            language,
            timezone,
        } => cmd::init::run(advanced, non_interactive, language, timezone),
        Cmd::Config { op } => cmd::config::run(op),
        Cmd::Wrapper { op } => cmd::wrapper::run(op),
        Cmd::Doctor => cmd::doctor::run(),
        Cmd::DbPath => cmd::db_path::run(),
        Cmd::Window(args) => cmd::window::run(args.tz, args.period),
        Cmd::Histogram(args) => {
            cmd::histogram::run(args.db, args.tz, args.start, args.end, args.agent)
        }
        Cmd::DailyTotals(args) => {
            cmd::daily_totals::run(args.db, args.tz, args.start, args.end)
        }
        Cmd::PerDayFiles(args) => {
            cmd::per_day_files::run(args.db, args.tz, args.start, args.end)
        }
        Cmd::Sqlite(args) => cmd::sqlite::run(args.db, args.sql),
    }
}
