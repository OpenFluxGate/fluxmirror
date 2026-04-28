// fluxmirror today — daily activity report.
//
// Thin wrapper around `cmd::report::day` (shared with the `yesterday`
// subcommand): pick the right one-day window, hand it to the shared
// collector, and ask the shared renderer for the today-flavoured title.
//
// Scope decision: the legacy slash command had a "Step 2 inference"
// layer that asked the model to label lifecycle stages, effort signals,
// and iterative refinement patterns. M1 deliberately drops that layer
// from the runtime path — the binary outputs FACTS only (numbers, file
// lists, time clusters). If model-summarised paragraphs ever come back,
// they ship as a separate `--with-summary` flag in a future slice.
//
// M5.4 adds `--format html`: a self-contained day-shaped digest card
// rendered through `cmd::report::html_day`.

use std::path::PathBuf;
use std::process::ExitCode;

use crate::cmd::util::{err_exit2, open_db_readonly, parse_tz, scrub_for_output};
use crate::cmd::window::today_range;
use fluxmirror_core::report::pack;

use super::day::{collect_day, render_human, DayLabel};
use super::html_day::{render_day_card, DayCardKind};
use super::html_io::{emit_html, generated_footer};
use super::ReportFormat;

/// Args for `cmd::report::today::run`. Mirrors `AgentsArgs` so the
/// clap layer in `main.rs` can build it from a parallel CLI struct.
pub struct TodayArgs {
    /// SQLite events database (read-only).
    pub db: PathBuf,
    /// IANA timezone for the today-window calculation.
    pub tz: String,
    /// Language code (canonical: english | korean | japanese | chinese).
    pub lang: String,
    /// Output format. M5.4 ships `Human` and `Html`.
    pub format: ReportFormat,
    /// Optional output path for `--format html`. `Some(-)` routes to
    /// stdout; `Some(path)` writes to that path; `None` triggers the
    /// auto-out path (`/tmp/fluxmirror-today-<timestamp>.html`).
    pub out: Option<PathBuf>,
}

pub fn run(args: TodayArgs) -> ExitCode {
    match args.format {
        ReportFormat::Human | ReportFormat::Html => {}
        ReportFormat::Json | ReportFormat::Markdown => {
            eprintln!(
                "fluxmirror today: --format {} not yet implemented for this report",
                args.format
            );
            return ExitCode::from(2);
        }
    }

    let tz = match parse_tz(&args.tz) {
        Ok(t) => t,
        Err(e) => return err_exit2(format!("fluxmirror today: {e}")),
    };
    let (target_date, start_utc, end_utc) = match today_range(tz) {
        Ok(r) => r,
        Err(e) => return err_exit2(format!("fluxmirror today: {e}")),
    };
    let conn = match open_db_readonly(&args.db) {
        Ok(c) => c,
        Err(e) => return err_exit2(format!("fluxmirror today: {e}")),
    };

    let day = match collect_day(&conn, tz, start_utc, end_utc, None) {
        Ok(d) => d,
        Err(e) => return err_exit2(format!("fluxmirror today: {e}")),
    };

    let lp = pack(&args.lang);

    if matches!(args.format, ReportFormat::Html) {
        let html = render_day_card(
            &day,
            target_date,
            &args.tz,
            DayCardKind::Today,
            lp,
            &generated_footer(),
        );
        return emit_html("today", html, args.out.as_deref());
    }

    let report = render_human(lp, &args.tz, target_date, &day, DayLabel::Today);
    print!("{}", scrub_for_output(&report));
    ExitCode::SUCCESS
}
