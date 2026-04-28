// fluxmirror about — explain what fluxmirror is and list subcommands.
//
// Pure metadata: no DB query, no file I/O beyond resolving the
// platform-default data paths. The rendered output stays deterministic
// and translates cleanly under --lang ko / --lang ja / --lang zh.
//
// The slash-command list is a static table inside this module — we
// deliberately don't walk the filesystem so the report never depends on
// where (or whether) the slash-command files are installed.

use std::path::PathBuf;
use std::process::ExitCode;

use fluxmirror_core::paths;
use fluxmirror_core::report::{pack, LangPack};

use crate::cmd::util::scrub_for_output;

use super::ReportFormat;

/// CLI args for the about subcommand.
pub struct AboutArgs {
    /// Override the printed DB path. Defaults to `paths::default_db_path()`.
    pub db: Option<PathBuf>,
    pub lang: String,
    pub format: ReportFormat,
}

pub fn run(args: AboutArgs) -> ExitCode {
    if !matches!(args.format, ReportFormat::Human) {
        // M5 ships --format html for the `week` subcommand only.
        eprintln!(
            "fluxmirror about: --format {} not yet implemented for this report",
            args.format
        );
        return ExitCode::from(2);
    }

    let lp = pack(&args.lang);
    let db = args.db.unwrap_or_else(paths::default_db_path);
    let log_path = paths::config_dir().join("hook-errors.log");
    let report = render_human(lp, &db.display().to_string(), &log_path.display().to_string());
    print!("{}", scrub_for_output(&report));
    ExitCode::SUCCESS
}

/// Static list of `(name, one-line description)` pairs covering every
/// /fluxmirror:* slash command shipped today. Hand-maintained — adding
/// a new slash command means adding a row here so `about` stays an
/// honest index. Keeps the report independent of filesystem layout.
const COMMAND_TABLE: &[(&str, &str)] = &[
    ("today", "Summarize today's AI agent activity."),
    ("yesterday", "Summarize yesterday's AI agent activity."),
    ("week", "Summarize the last 7 days."),
    ("compare", "Compare today vs yesterday side-by-side."),
    ("agents", "Per-agent quick stats for the past 7 days."),
    ("agent", "Single-agent filtered report (today | yesterday | week)."),
    ("about", "Explain what fluxmirror is and list available commands."),
];

fn render_human(lp: &LangPack, db_path: &str, log_path: &str) -> String {
    let mut out = String::new();
    out.push_str(&format!("# {}\n\n", lp.about_title));
    out.push_str(lp.about_blurb);
    out.push_str("\n\n");

    out.push_str(&format!("## {}\n\n", lp.about_commands_heading));
    for (name, desc) in COMMAND_TABLE {
        out.push_str(&format!("- /fluxmirror:{} — {}\n", name, desc));
    }
    out.push('\n');

    out.push_str(&format!("## {}\n\n", lp.about_paths_heading));
    out.push_str(&format!("- {}: {}\n", lp.about_db_label, db_path));
    out.push_str(&format!("- {}: {}\n", lp.about_log_label, log_path));

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_lists_all_seven_command_names() {
        let lp = pack("english");
        let s = render_human(lp, "/db.sqlite", "/var/log/flux.log");
        for (name, _) in COMMAND_TABLE {
            assert!(
                s.contains(&format!("/fluxmirror:{}", name)),
                "missing /fluxmirror:{} in:\n{}",
                name,
                s
            );
        }
        assert!(s.contains("About FluxMirror"));
        assert!(s.contains("/db.sqlite"));
        assert!(s.contains("/var/log/flux.log"));
    }

    #[test]
    fn korean_keeps_command_names_but_translates_section_headings() {
        let lp = pack("korean");
        let s = render_human(lp, "/db.sqlite", "/var/log/flux.log");
        for (name, _) in COMMAND_TABLE {
            assert!(
                s.contains(&format!("/fluxmirror:{}", name)),
                "ko output missing /fluxmirror:{} in:\n{}",
                name,
                s
            );
        }
        assert!(s.contains("FluxMirror 소개"));
        assert!(s.contains("사용 가능한 명령"));
        assert!(s.contains("데이터 위치"));
    }

    #[test]
    fn command_table_has_seven_entries() {
        assert_eq!(COMMAND_TABLE.len(), 7);
    }
}
