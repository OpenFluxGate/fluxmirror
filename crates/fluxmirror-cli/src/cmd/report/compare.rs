// fluxmirror compare — today vs yesterday side-by-side.
//
// Calls the shared `cmd::report::day::collect_day` once per side, then
// renders a single comparison table with a Δ% column. Highlights any
// metric whose absolute change is at least the COMPARE_HIGHLIGHT_PCT
// threshold with an arrow indicator (↑ / ↓).

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::ExitCode;

use chrono::NaiveDate;

use crate::cmd::util::{err_exit2, open_db_readonly, parse_tz, scrub_for_output};
use crate::cmd::window::{day_range, today_range};
use fluxmirror_core::report::{pack, LangPack};

use super::day::{collect_day, DayStats};
use super::html_day::render_compare_card;
use super::html_io::{emit_html, generated_footer};
use super::ReportFormat;

/// Threshold (percent) at which the Δ column gets an arrow indicator.
const COMPARE_HIGHLIGHT_PCT: u32 = 50;

/// CLI args for the compare subcommand.
pub struct CompareArgs {
    pub db: PathBuf,
    pub tz: String,
    pub lang: String,
    pub format: ReportFormat,
    pub out: Option<PathBuf>,
}

pub fn run(args: CompareArgs) -> ExitCode {
    match args.format {
        ReportFormat::Human | ReportFormat::Html => {}
        ReportFormat::Json | ReportFormat::Markdown => {
            eprintln!(
                "fluxmirror compare: --format {} not yet implemented for this report",
                args.format
            );
            return ExitCode::from(2);
        }
    }

    let tz = match parse_tz(&args.tz) {
        Ok(t) => t,
        Err(e) => return err_exit2(format!("fluxmirror compare: {e}")),
    };
    let (today_date, today_start, today_end) = match today_range(tz) {
        Ok(r) => r,
        Err(e) => return err_exit2(format!("fluxmirror compare: today range: {e}")),
    };
    let (yest_date, yest_start, yest_end) = match day_range(tz, -1) {
        Ok(r) => r,
        Err(e) => return err_exit2(format!("fluxmirror compare: yesterday range: {e}")),
    };
    let conn = match open_db_readonly(&args.db) {
        Ok(c) => c,
        Err(e) => return err_exit2(format!("fluxmirror compare: {e}")),
    };

    let today = match collect_day(&conn, tz, today_start, today_end, None) {
        Ok(d) => d,
        Err(e) => return err_exit2(format!("fluxmirror compare: {e}")),
    };
    let yesterday = match collect_day(&conn, tz, yest_start, yest_end, None) {
        Ok(d) => d,
        Err(e) => return err_exit2(format!("fluxmirror compare: {e}")),
    };

    let lp = pack(&args.lang);

    if matches!(args.format, ReportFormat::Html) {
        let html = render_compare_card(
            &today,
            &yesterday,
            today_date,
            yest_date,
            &args.tz,
            lp,
            &generated_footer(),
        );
        return emit_html("compare", html, args.out.as_deref());
    }

    let report = render_human(lp, &args.tz, today_date, yest_date, &today, &yesterday);
    print!("{}", scrub_for_output(&report));
    ExitCode::SUCCESS
}

/// Six-row metrics view, in the order they appear in the table.
#[derive(Debug, Default, Copy, Clone)]
struct Metrics {
    total: u64,
    edits: u64,
    reads: u64,
    shells: u64,
    distinct_files: u64,
    distinct_cwds: u64,
}

impl Metrics {
    fn from_day(day: &DayStats) -> Self {
        // shell rows are kept in `shells` (Vec<ShellRow>) — the count is
        // its length. distinct files / cwds come from the shared sets.
        let distinct_files = day.distinct_files.len() as u64;
        let distinct_cwds: u64 = day.cwds.keys().collect::<BTreeSet<_>>().len() as u64;
        Metrics {
            total: day.total_events,
            edits: day.writes_total,
            reads: day.reads_total,
            shells: day.shells.len() as u64,
            distinct_files,
            distinct_cwds,
        }
    }

    /// Iterate the six metrics in render order so the table assembly
    /// loop stays declarative.
    fn pairs(&self) -> [u64; 6] {
        [
            self.total,
            self.edits,
            self.reads,
            self.shells,
            self.distinct_files,
            self.distinct_cwds,
        ]
    }
}

fn render_human(
    lp: &LangPack,
    tz_name: &str,
    today_date: NaiveDate,
    yest_date: NaiveDate,
    today: &DayStats,
    yesterday: &DayStats,
) -> String {
    let t = Metrics::from_day(today);
    let y = Metrics::from_day(yesterday);

    if t.total == 0 && y.total == 0 {
        return format!("{}\n", lp.compare_no_data);
    }

    let mut out = String::new();
    out.push_str(&format!(
        "# {} ({} vs {}, {})\n\n",
        lp.compare_title,
        today_date.format("%Y-%m-%d"),
        yest_date.format("%Y-%m-%d"),
        tz_name
    ));

    let cols = lp.compare_columns;
    out.push_str(&format!(
        "| {} | {} | {} | {} |\n",
        cols[0], cols[1], cols[2], cols[3]
    ));
    out.push_str("|---|---|---|---|\n");

    let labels = lp.compare_metric_labels;
    let t_pairs = t.pairs();
    let y_pairs = y.pairs();
    for (i, label) in labels.iter().enumerate() {
        let (today_v, yest_v) = (t_pairs[i], y_pairs[i]);
        let delta = format_delta(today_v, yest_v);
        out.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            label, today_v, yest_v, delta
        ));
    }
    out.push('\n');

    let bullets = compute_insights(lp, t.total, y.total);
    if !bullets.is_empty() {
        // Reuse the today insights heading — keeps the section title
        // consistent with the other reports.
        out.push_str(&format!("## {}\n\n", lp.today_insights_heading));
        for b in &bullets {
            out.push_str(&format!("- {}\n", b));
        }
    }
    out
}

/// Format a Δ cell. Encodes:
/// - `n/a` when yesterday is 0 and today is non-zero (no baseline).
/// - `+pct%` / `-pct%` otherwise.
/// - Trailing arrow ↑ / ↓ when |pct| >= COMPARE_HIGHLIGHT_PCT.
fn format_delta(today: u64, yest: u64) -> String {
    if yest == 0 && today == 0 {
        return "0%".to_string();
    }
    if yest == 0 {
        return "n/a".to_string();
    }
    let diff = today as i128 - yest as i128;
    let pct = (diff * 100) / yest as i128;
    let arrow = if pct.unsigned_abs() >= COMPARE_HIGHLIGHT_PCT as u128 {
        if pct > 0 {
            " ↑"
        } else if pct < 0 {
            " ↓"
        } else {
            ""
        }
    } else {
        ""
    };
    if pct >= 0 {
        format!("+{}%{}", pct, arrow)
    } else {
        format!("{}%{}", pct, arrow)
    }
}

/// Three-bullet (max) insight bullets for the compare report.
fn compute_insights(lp: &LangPack, today_total: u64, yest_total: u64) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    match (today_total, yest_total) {
        (0, 0) => {
            out.push(lp.compare_insight_both_quiet.to_string());
        }
        (n, 0) => {
            out.push(
                lp.compare_insight_only_today
                    .replace("{n}", &n.to_string()),
            );
        }
        (0, n) => {
            out.push(
                lp.compare_insight_only_yesterday
                    .replace("{n}", &n.to_string()),
            );
        }
        (t, y) => {
            let diff = t as i128 - y as i128;
            let pct = (diff * 100) / y as i128;
            let direction = if pct >= 0 {
                lp.compare_word_up
            } else {
                lp.compare_word_down
            };
            out.push(
                lp.compare_insight_calls_trend
                    .replace("{direction}", direction)
                    .replace("{pct}", &pct.unsigned_abs().to_string()),
            );
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lp() -> &'static LangPack {
        pack("english")
    }

    #[test]
    fn format_delta_plus_arrow_above_threshold() {
        // +100% from 5 → 10 should fire the up-arrow.
        assert_eq!(format_delta(10, 5), "+100% ↑");
    }

    #[test]
    fn format_delta_minus_arrow_above_threshold() {
        // 10 → 4 = -60% → down-arrow.
        assert_eq!(format_delta(4, 10), "-60% ↓");
    }

    #[test]
    fn format_delta_under_threshold_skips_arrow() {
        // 10 → 12 = +20% → no arrow.
        assert_eq!(format_delta(12, 10), "+20%");
    }

    #[test]
    fn format_delta_zero_baseline_renders_na() {
        assert_eq!(format_delta(5, 0), "n/a");
    }

    #[test]
    fn format_delta_both_zero_renders_zero_pct() {
        assert_eq!(format_delta(0, 0), "0%");
    }

    #[test]
    fn render_human_emits_no_data_when_both_zero() {
        let day = DayStats::default();
        let s = render_human(
            lp(),
            "UTC",
            NaiveDate::from_ymd_opt(2026, 4, 26).unwrap(),
            NaiveDate::from_ymd_opt(2026, 4, 25).unwrap(),
            &day,
            &day,
        );
        assert!(s.contains("Not enough activity to compare"));
    }

    #[test]
    fn render_human_includes_table_with_delta_column() {
        let mut today = DayStats::default();
        today.total_events = 10;
        today.writes_total = 4;
        today.reads_total = 3;
        let mut yest = DayStats::default();
        yest.total_events = 5;
        yest.writes_total = 2;
        yest.reads_total = 1;

        let s = render_human(
            lp(),
            "UTC",
            NaiveDate::from_ymd_opt(2026, 4, 26).unwrap(),
            NaiveDate::from_ymd_opt(2026, 4, 25).unwrap(),
            &today,
            &yest,
        );
        assert!(s.contains("Today vs Yesterday"), "missing title:\n{s}");
        // Δ column header is present.
        assert!(s.contains("| Δ |"), "missing delta header:\n{s}");
        // Calls row: today=10, yesterday=5 → +100% ↑
        assert!(s.contains("+100%"), "missing +100%:\n{s}");
        assert!(s.contains("↑"), "missing up arrow:\n{s}");
        assert!(
            s.contains("Calls: today is up 100%"),
            "missing calls-trend insight:\n{s}"
        );
    }

    #[test]
    fn only_today_branch_used_when_yesterday_zero() {
        let mut today = DayStats::default();
        today.total_events = 7;
        let yest = DayStats::default();
        let s = render_human(
            lp(),
            "UTC",
            NaiveDate::from_ymd_opt(2026, 4, 26).unwrap(),
            NaiveDate::from_ymd_opt(2026, 4, 25).unwrap(),
            &today,
            &yest,
        );
        assert!(
            s.contains("Yesterday had no activity"),
            "missing only-today insight:\n{s}"
        );
        // Delta column for total should be "n/a"
        assert!(s.contains("n/a"), "missing n/a delta:\n{s}");
    }

    #[test]
    fn korean_keeps_arrow_chars_but_translates_words() {
        let lp = pack("korean");
        let mut today = DayStats::default();
        today.total_events = 10;
        let mut yest = DayStats::default();
        yest.total_events = 5;
        let s = render_human(
            lp,
            "Asia/Seoul",
            NaiveDate::from_ymd_opt(2026, 4, 26).unwrap(),
            NaiveDate::from_ymd_opt(2026, 4, 25).unwrap(),
            &today,
            &yest,
        );
        assert!(s.contains("오늘 vs 어제"), "missing ko title:\n{s}");
        assert!(s.contains("↑"), "arrow char missing:\n{s}");
        assert!(s.contains("증가"), "missing ko 'up' word:\n{s}");
    }
}
