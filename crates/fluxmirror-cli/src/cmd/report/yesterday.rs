// fluxmirror yesterday — yesterday's activity report.
//
// Mechanically a copy of `today` with `day_offset = -1`. The shared
// engine in `cmd::report::day` does the aggregation and rendering; this
// module only picks the window and the localized title flavour.

use std::path::PathBuf;
use std::process::ExitCode;

use crate::cmd::util::{err_exit2, open_db_readonly, parse_tz};
use crate::cmd::window::day_range;
use fluxmirror_core::report::pack;

use super::day::{collect_day, render_human, DayLabel};
use super::ReportFormat;

/// Args for `cmd::report::yesterday::run`. Same shape as `TodayArgs`.
pub struct YesterdayArgs {
    pub db: PathBuf,
    pub tz: String,
    pub lang: String,
    pub format: ReportFormat,
}

pub fn run(args: YesterdayArgs) -> ExitCode {
    if !matches!(args.format, ReportFormat::Human) {
        // M5 ships --format html for the `week` subcommand only.
        eprintln!(
            "fluxmirror yesterday: --format {} not yet implemented for this report",
            args.format
        );
        return ExitCode::from(2);
    }

    let tz = match parse_tz(&args.tz) {
        Ok(t) => t,
        Err(e) => return err_exit2(format!("fluxmirror yesterday: {e}")),
    };
    let (target_date, start_utc, end_utc) = match day_range(tz, -1) {
        Ok(r) => r,
        Err(e) => return err_exit2(format!("fluxmirror yesterday: {e}")),
    };
    let conn = match open_db_readonly(&args.db) {
        Ok(c) => c,
        Err(e) => return err_exit2(format!("fluxmirror yesterday: {e}")),
    };

    let day = match collect_day(&conn, tz, start_utc, end_utc, None) {
        Ok(d) => d,
        Err(e) => return err_exit2(format!("fluxmirror yesterday: {e}")),
    };

    let lp = pack(&args.lang);
    let report = render_human(lp, &args.tz, target_date, &day, DayLabel::Yesterday);
    print!("{}", report);
    ExitCode::SUCCESS
}
