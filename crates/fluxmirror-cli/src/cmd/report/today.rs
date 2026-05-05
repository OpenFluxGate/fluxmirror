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
use fluxmirror_core::report::data as core_data;
use fluxmirror_core::report::dto::{DailyNarrative, NarrativeSource, TodayData, WindowRange};
use fluxmirror_core::report::pack;
use fluxmirror_core::Config;
use fluxmirror_store::SqliteStore;

use super::day::{render_human, today_data_to_day_stats, DayLabel};
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

    let range = WindowRange {
        start_utc,
        end_utc,
        anchor_date: target_date,
        tz: tz.name().to_string(),
    };
    let mut today_data: TodayData = match core_data::collect_today(&conn, &tz, range, None) {
        Ok(d) => d,
        Err(e) => return err_exit2(format!("fluxmirror today: {e}")),
    };

    // Phase 4 M-A2 — overlay the daily narrative paragraph. The synthesis
    // wrapper is best-effort: provider="off", missing API key, budget hit,
    // or transient network error all fall back to the heuristic so this
    // path never blocks the report.
    today_data.narrative = compute_narrative(&args.db, &today_data);

    let lp = pack(&args.lang);
    let day = today_data_to_day_stats(today_data.clone());

    if matches!(args.format, ReportFormat::Html) {
        let html = render_day_card(
            &day,
            target_date,
            &args.tz,
            DayCardKind::Today,
            lp,
            &generated_footer(),
            Some(&today_data.narrative),
        );
        return emit_html("today", html, args.out.as_deref());
    }

    let body = render_human(lp, &args.tz, target_date, &day, DayLabel::Today);
    let report = prepend_narrative(&body, &today_data.narrative);
    print!("{}", scrub_for_output(&report));
    ExitCode::SUCCESS
}

/// Build the daily narrative for this CLI invocation. Matches the
/// studio's wiring exactly so the same DB + config produces the same
/// paragraph (cache hit on the second surface).
fn compute_narrative(db_path: &PathBuf, today: &TodayData) -> DailyNarrative {
    let cfg = Config::load().unwrap_or_default();
    let store = if cfg.ai.provider == "off" {
        None
    } else {
        SqliteStore::open(db_path).ok()
    };
    fluxmirror_ai::synthesise_daily(store.as_ref(), &cfg, today)
}

/// Prepend the `## Narrative` section to a rendered today/yesterday
/// markdown body. The narrative section is positioned above the title's
/// existing content but below the leading `# Today's Work …` heading
/// so the heading still leads the document.
fn prepend_narrative(body: &str, narrative: &DailyNarrative) -> String {
    if narrative.paragraph.is_empty() {
        return body.to_string();
    }
    let footnote = if matches!(narrative.source, NarrativeSource::Heuristic) {
        "\n\n_(estimate)_"
    } else {
        ""
    };
    let block = format!("## Narrative\n\n{}{}\n\n", narrative.paragraph, footnote);

    // Insert after the leading `# ...` title line if present, otherwise
    // prepend.
    if let Some(first_break) = body.find("\n\n") {
        let (title, rest) = body.split_at(first_break + 2);
        format!("{title}{block}{rest}")
    } else {
        format!("{block}{body}")
    }
}
