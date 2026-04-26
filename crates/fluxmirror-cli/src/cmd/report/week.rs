// fluxmirror week — 7-day rollup report.
//
// Different shape from today/yesterday: covers a week range (inclusive
// today) and surfaces per-day totals plus a day-distribution chart in
// place of the hour-distribution chart. Top files lists are widened
// (top 30 edited / top 15 read) since a week has more spread.
//
// Sections:
//   1. Title `# Last 7 Days (start ~ end, tz)`
//   2. Per-agent calls / sessions
//   3. Per-day totals (Date | Calls), one row per day in window
//   4. Top edited files (top 30) — Path | Tool | Count
//   5. Top read files (top 15) — Path | Count
//   6. Tool mix
//   7. Working directories (top 10)
//   8. Day distribution — bar chart of calls per day
//   9. Insights — three deterministic bullets max:
//      - Most productive day
//      - Days active: <N>/7
//      - Cross-project: <N> distinct cwds with ≥5 calls

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;
use std::process::ExitCode;

use chrono::{DateTime, Duration, NaiveDate, Utc};
use chrono_tz::Tz;
use rusqlite::Connection;

use crate::cmd::util::{err_exit2, open_db_readonly, parse_tz};
use crate::cmd::window::week_range;
use fluxmirror_core::report::{pack, LangPack};

use super::tools::{is_read, is_shell, is_write};
use super::ReportFormat;

/// Maximum rows in the "files written or edited" table — week sees more
/// spread than a single day so we widen versus today's 20.
const FILES_EDITED_LIMIT: usize = 30;
/// Maximum rows in the "files only read" table.
const FILES_READ_LIMIT: usize = 15;
/// Maximum rows in the working-directories table.
const CWDS_LIMIT: usize = 10;
/// Maximum width (chars) of the bar in the day-distribution chart.
const DAY_BAR_WIDTH: u32 = 30;
/// A working directory needs at least this many calls to count toward
/// the cross-project insight.
const CROSS_PROJECT_CWD_MIN_CALLS: u64 = 5;
/// Below this number of total events, emit the "limited activity" line
/// and exit 0.
const LIMITED_ACTIVITY_THRESHOLD: u64 = 5;

/// CLI args for the week subcommand.
pub struct WeekArgs {
    pub db: PathBuf,
    pub tz: String,
    pub lang: String,
    pub format: ReportFormat,
}

pub fn run(args: WeekArgs) -> ExitCode {
    if !matches!(args.format, ReportFormat::Human) {
        eprintln!(
            "fluxmirror week: --format {} not yet implemented (M1 ships --format human only)",
            args.format
        );
        return ExitCode::from(2);
    }

    let tz = match parse_tz(&args.tz) {
        Ok(t) => t,
        Err(e) => return err_exit2(format!("fluxmirror week: {e}")),
    };
    let (week_start, week_end, start_utc, end_utc) = match week_range(tz) {
        Ok(r) => r,
        Err(e) => return err_exit2(format!("fluxmirror week: {e}")),
    };
    let conn = match open_db_readonly(&args.db) {
        Ok(c) => c,
        Err(e) => return err_exit2(format!("fluxmirror week: {e}")),
    };

    let stats = match collect_week(&conn, tz, week_start, start_utc, end_utc) {
        Ok(s) => s,
        Err(e) => return err_exit2(format!("fluxmirror week: {e}")),
    };

    let lp = pack(&args.lang);
    let report = render_human(lp, &args.tz, week_start, week_end, &stats);
    print!("{}", report);
    ExitCode::SUCCESS
}

/// Per-agent row in the week activity table.
#[derive(Debug, Default, Clone)]
pub(crate) struct AgentRow {
    pub calls: u64,
    pub sessions: BTreeSet<String>,
}

/// All week aggregates. The fields mirror the today report's `DayStats`
/// for the columns the two share, plus a `daily_calls` map and a
/// `days_in_window` ordered list driving both the per-day-totals table
/// and the day-distribution chart.
#[derive(Debug, Default)]
pub(crate) struct WeekStats {
    pub total_events: u64,
    pub agents: BTreeMap<String, AgentRow>,
    pub files_edited: HashMap<(String, String), u64>,
    pub files_read: HashMap<String, u64>,
    pub cwds: HashMap<String, u64>,
    pub tool_mix: HashMap<String, u64>,
    /// Inclusive list of local dates in the window, in chronological
    /// order. Built up front so zero-event days are visible.
    pub days_in_window: Vec<NaiveDate>,
    /// local-date -> call count for that day.
    pub daily_calls: HashMap<NaiveDate, u64>,
    #[allow(dead_code)]
    pub reads_total: u64,
    #[allow(dead_code)]
    pub writes_total: u64,
}

/// Build the day list and run the aggregation in one pass over the
/// window's `agent_events` rows.
pub(crate) fn collect_week(
    conn: &Connection,
    tz: Tz,
    week_start: NaiveDate,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Result<WeekStats, String> {
    let start_str = start.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let end_str = end.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

    let mut stats = WeekStats::default();

    // Build the inclusive 7-day list anchored at week_start; pre-seed
    // daily_calls with zeroes so a day with no events still shows up.
    for i in 0..7 {
        let d = week_start + Duration::days(i);
        stats.days_in_window.push(d);
        stats.daily_calls.insert(d, 0);
    }

    let mut stmt = conn
        .prepare(
            "SELECT ts, agent, COALESCE(session, '') AS session, \
                    COALESCE(tool, '') AS tool, COALESCE(detail, '') AS detail, \
                    COALESCE(cwd, '') AS cwd \
             FROM agent_events WHERE ts >= ?1 AND ts < ?2",
        )
        .map_err(|e| format!("prepare(events): {e}"))?;
    let rows = stmt
        .query_map([&start_str, &end_str], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
            ))
        })
        .map_err(|e| format!("query(events): {e}"))?;

    for row in rows {
        let (ts, agent, session, tool, detail, cwd) =
            row.map_err(|e| format!("row(events): {e}"))?;
        stats.total_events += 1;

        let entry = stats.agents.entry(agent).or_default();
        entry.calls += 1;
        if !session.is_empty() {
            entry.sessions.insert(session);
        }

        if !tool.is_empty() {
            *stats.tool_mix.entry(tool.clone()).or_default() += 1;
        }
        if !cwd.is_empty() {
            *stats.cwds.entry(cwd).or_default() += 1;
        }

        let tool_str = tool.as_str();
        if is_write(tool_str) && !detail.is_empty() {
            *stats
                .files_edited
                .entry((detail.clone(), tool.clone()))
                .or_default() += 1;
            stats.writes_total += 1;
        } else if is_read(tool_str) && !detail.is_empty() {
            *stats.files_read.entry(detail.clone()).or_default() += 1;
            stats.reads_total += 1;
        } else if is_shell(tool_str) {
            // shells aren't surfaced in the week report directly but we
            // still tally them toward total_events / tool_mix.
        } else if is_write(tool_str) {
            stats.writes_total += 1;
        } else if is_read(tool_str) {
            stats.reads_total += 1;
        }

        // Daily calls bucket — same parse-then-localize pattern as the
        // hour bucket in today.rs, but bucketed on local date.
        if let Ok(dt) = DateTime::parse_from_rfc3339(&ts) {
            let local_date = dt.with_timezone(&tz).date_naive();
            if let Some(c) = stats.daily_calls.get_mut(&local_date) {
                *c += 1;
            }
        }
    }

    Ok(stats)
}

fn render_human(
    lp: &LangPack,
    tz_name: &str,
    week_start: NaiveDate,
    week_end: NaiveDate,
    stats: &WeekStats,
) -> String {
    if stats.total_events < LIMITED_ACTIVITY_THRESHOLD {
        return format!("{}\n", lp.week_no_data);
    }

    let mut out = String::new();

    // Title: `# Last 7 Days (YYYY-MM-DD ~ YYYY-MM-DD, tz)`.
    out.push_str(&format!(
        "# {} ({} ~ {}, {})\n\n",
        lp.week_title,
        week_start.format("%Y-%m-%d"),
        week_end.format("%Y-%m-%d"),
        tz_name
    ));

    render_activity(&mut out, lp, stats);
    render_daily_totals(&mut out, lp, stats);
    render_files_edited(&mut out, lp, stats);
    render_files_read(&mut out, lp, stats);
    render_tool_mix(&mut out, lp, stats);
    render_cwds(&mut out, lp, stats);
    render_day_distribution(&mut out, lp, stats);
    render_insights(&mut out, lp, stats);

    out
}

fn render_activity(out: &mut String, lp: &LangPack, stats: &WeekStats) {
    out.push_str(&format!("## {}\n\n", lp.today_activity_heading));
    let cols = lp.today_columns_calls_sessions;
    out.push_str(&format!("| {} | {} | {} |\n", cols[0], cols[1], cols[2]));
    out.push_str("|---|---|---|\n");
    let mut rows: Vec<(&String, &AgentRow)> = stats.agents.iter().collect();
    rows.sort_by(|a, b| b.1.calls.cmp(&a.1.calls).then_with(|| a.0.cmp(b.0)));
    for (agent, row) in &rows {
        out.push_str(&format!(
            "| {} | {} | {} |\n",
            agent,
            row.calls,
            row.sessions.len()
        ));
    }
    out.push('\n');
}

fn render_daily_totals(out: &mut String, lp: &LangPack, stats: &WeekStats) {
    if stats.days_in_window.is_empty() {
        return;
    }
    out.push_str(&format!("## {}\n\n", lp.week_daily_totals_heading));
    let cols = lp.week_columns_date_calls;
    out.push_str(&format!("| {} | {} |\n", cols[0], cols[1]));
    out.push_str("|---|---|\n");
    for d in &stats.days_in_window {
        let n = stats.daily_calls.get(d).copied().unwrap_or(0);
        out.push_str(&format!("| {} ({}) | {} |\n", d.format("%Y-%m-%d"), d.format("%a"), n));
    }
    out.push('\n');
}

fn render_files_edited(out: &mut String, lp: &LangPack, stats: &WeekStats) {
    if stats.files_edited.is_empty() {
        return;
    }
    out.push_str(&format!("## {}\n\n", lp.today_files_edited_heading));
    let cols = lp.today_columns_file_tool_count;
    out.push_str(&format!("| {} | {} | {} |\n", cols[0], cols[1], cols[2]));
    out.push_str("|---|---|---|\n");
    let mut rows: Vec<(&(String, String), &u64)> = stats.files_edited.iter().collect();
    rows.sort_by(|a, b| {
        b.1.cmp(a.1)
            .then_with(|| a.0 .0.cmp(&b.0 .0))
            .then_with(|| a.0 .1.cmp(&b.0 .1))
    });
    for ((path, tool), n) in rows.iter().take(FILES_EDITED_LIMIT) {
        out.push_str(&format!("| {} | {} | {} |\n", path, tool, n));
    }
    out.push('\n');
}

fn render_files_read(out: &mut String, lp: &LangPack, stats: &WeekStats) {
    if stats.files_read.is_empty() {
        return;
    }
    out.push_str(&format!("## {}\n\n", lp.today_files_read_heading));
    let cols = lp.today_columns_path_count;
    out.push_str(&format!("| {} | {} |\n", cols[0], cols[1]));
    out.push_str("|---|---|\n");
    let mut rows: Vec<(&String, &u64)> = stats.files_read.iter().collect();
    rows.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
    for (path, n) in rows.iter().take(FILES_READ_LIMIT) {
        out.push_str(&format!("| {} | {} |\n", path, n));
    }
    out.push('\n');
}

fn render_tool_mix(out: &mut String, lp: &LangPack, stats: &WeekStats) {
    if stats.tool_mix.is_empty() {
        return;
    }
    out.push_str(&format!("## {}\n\n", lp.today_tool_mix_heading));
    let cols = lp.today_columns_tool_count;
    out.push_str(&format!("| {} | {} |\n", cols[0], cols[1]));
    out.push_str("|---|---|\n");
    let mut rows: Vec<(&String, &u64)> = stats.tool_mix.iter().collect();
    rows.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
    for (tool, n) in &rows {
        out.push_str(&format!("| {} | {} |\n", tool, n));
    }
    out.push('\n');
}

fn render_cwds(out: &mut String, lp: &LangPack, stats: &WeekStats) {
    if stats.cwds.is_empty() {
        return;
    }
    out.push_str(&format!("## {}\n\n", lp.today_cwds_heading));
    let cols = lp.today_columns_path_count;
    out.push_str(&format!("| {} | {} |\n", cols[0], cols[1]));
    out.push_str("|---|---|\n");
    let mut rows: Vec<(&String, &u64)> = stats.cwds.iter().collect();
    rows.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
    for (path, n) in rows.iter().take(CWDS_LIMIT) {
        out.push_str(&format!("| {} | {} |\n", path, n));
    }
    out.push('\n');
}

fn render_day_distribution(out: &mut String, lp: &LangPack, stats: &WeekStats) {
    let max = stats
        .daily_calls
        .values()
        .copied()
        .max()
        .unwrap_or(0);
    if max == 0 {
        return;
    }
    out.push_str(&format!("## {}\n\n", lp.week_day_distribution_heading));
    out.push_str("```\n");
    for d in &stats.days_in_window {
        let n = stats.daily_calls.get(d).copied().unwrap_or(0);
        let bar_len = if max == 0 {
            0
        } else {
            ((n as u128 * DAY_BAR_WIDTH as u128) / max as u128) as usize
        };
        // Always show at least one char for non-zero days.
        let bar_len = if n > 0 { bar_len.max(1) } else { 0 };
        let bar: String = std::iter::repeat('█').take(bar_len).collect();
        out.push_str(&format!(
            "{} ({}) {} {}\n",
            d.format("%Y-%m-%d"),
            d.format("%a"),
            bar,
            n
        ));
    }
    out.push_str("```\n\n");
}

fn render_insights(out: &mut String, lp: &LangPack, stats: &WeekStats) {
    let bullets = compute_insights(lp, stats);
    if bullets.is_empty() {
        return;
    }
    out.push_str(&format!("## {}\n\n", lp.today_insights_heading));
    for b in &bullets {
        out.push_str(&format!("- {}\n", b));
    }
}

fn compute_insights(lp: &LangPack, stats: &WeekStats) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();

    // Rule 1: most productive day. Ties resolve to the earliest date for
    // determinism (BTreeMap-style — we walk the ordered list and keep
    // the strict-greater winner).
    let mut top: Option<(NaiveDate, u64)> = None;
    for d in &stats.days_in_window {
        let n = stats.daily_calls.get(d).copied().unwrap_or(0);
        match top {
            None => top = Some((*d, n)),
            Some((_, cur)) if n > cur => top = Some((*d, n)),
            _ => {}
        }
    }
    if let Some((date, n)) = top {
        if n > 0 {
            out.push(
                lp.week_insight_top_day
                    .replace("{date}", &date.format("%Y-%m-%d").to_string())
                    .replace("{n}", &n.to_string()),
            );
        }
    }

    // Rule 2: days active.
    let active_days: u64 = stats
        .daily_calls
        .values()
        .filter(|n| **n > 0)
        .count() as u64;
    out.push(
        lp.week_insight_active_days
            .replace("{n}", &active_days.to_string()),
    );

    // Rule 3: cross-project (analogue of today's multi-project rule).
    let cross_n: u64 = stats
        .cwds
        .values()
        .filter(|n| **n >= CROSS_PROJECT_CWD_MIN_CALLS)
        .count() as u64;
    if cross_n >= 2 {
        out.push(
            lp.week_insight_cross_project
                .replace("{n}", &cross_n.to_string()),
        );
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use fluxmirror_store::SqliteStore;
    use rusqlite::params;
    use tempfile::TempDir;

    fn fixture_db(rows: &[(&str, &str, &str, &str, &str, &str)]) -> (TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("events.db");
        let _store = SqliteStore::open(&path).unwrap();
        let conn = Connection::open(&path).unwrap();
        for (ts, agent, tool, session, detail, cwd) in rows {
            conn.execute(
                "INSERT INTO agent_events \
                 (ts, agent, session, tool, tool_canonical, tool_class, detail, \
                  cwd, host, user, schema_version, raw_json) \
                 VALUES (?1, ?2, ?3, ?4, ?4, 'Other', ?5, ?6, 'h', 'u', 1, '{}')",
                params![ts, agent, session, tool, detail, cwd],
            )
            .unwrap();
        }
        (dir, path)
    }

    #[test]
    fn collect_week_buckets_calls_per_day() {
        let (_d, path) = fixture_db(&[
            ("2026-04-21T01:00:00Z", "claude-code", "Edit", "s1", "a", "/p"),
            ("2026-04-22T02:00:00Z", "claude-code", "Edit", "s1", "a", "/p"),
            ("2026-04-22T03:00:00Z", "claude-code", "Edit", "s1", "a", "/p"),
            ("2026-04-25T04:00:00Z", "gemini-cli", "read_file", "g", "b", "/q"),
        ]);
        let conn = open_db_readonly(&path).unwrap();
        let tz: Tz = "UTC".parse().unwrap();
        let week_start = NaiveDate::from_ymd_opt(2026, 4, 21).unwrap();
        let start = "2026-04-21T00:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let end = "2026-04-28T00:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let stats = collect_week(&conn, tz, week_start, start, end).unwrap();

        assert_eq!(stats.total_events, 4);
        assert_eq!(stats.days_in_window.len(), 7);
        assert_eq!(stats.daily_calls.get(&week_start).copied(), Some(1));
        assert_eq!(
            stats.daily_calls.get(&NaiveDate::from_ymd_opt(2026, 4, 22).unwrap()).copied(),
            Some(2)
        );
        assert_eq!(
            stats.daily_calls.get(&NaiveDate::from_ymd_opt(2026, 4, 25).unwrap()).copied(),
            Some(1)
        );
        // Day with no rows in the window stays at 0 instead of being absent.
        assert_eq!(
            stats.daily_calls.get(&NaiveDate::from_ymd_opt(2026, 4, 27).unwrap()).copied(),
            Some(0)
        );
    }

    #[test]
    fn render_human_short_window_emits_no_data_line() {
        let lp = pack("english");
        let stats = WeekStats {
            total_events: 2,
            ..Default::default()
        };
        let s = render_human(
            lp,
            "UTC",
            NaiveDate::from_ymd_opt(2026, 4, 21).unwrap(),
            NaiveDate::from_ymd_opt(2026, 4, 27).unwrap(),
            &stats,
        );
        assert!(s.contains("Limited activity this week."), "got:\n{s}");
    }

    #[test]
    fn render_human_includes_title_per_day_and_distribution() {
        let lp = pack("english");
        let mut stats = WeekStats {
            total_events: 6,
            ..Default::default()
        };
        let week_start = NaiveDate::from_ymd_opt(2026, 4, 21).unwrap();
        for i in 0..7 {
            let d = week_start + Duration::days(i);
            stats.days_in_window.push(d);
            stats.daily_calls.insert(d, if i == 1 { 6 } else { 0 });
        }
        let mut row = AgentRow::default();
        row.calls = 6;
        row.sessions.insert("s1".into());
        stats.agents.insert("claude-code".into(), row);

        let s = render_human(
            lp,
            "UTC",
            week_start,
            NaiveDate::from_ymd_opt(2026, 4, 27).unwrap(),
            &stats,
        );
        assert!(s.contains("Last 7 Days"), "missing title:\n{s}");
        assert!(s.contains("2026-04-21"), "missing start date:\n{s}");
        assert!(s.contains("2026-04-27"), "missing end date:\n{s}");
        assert!(s.contains("Per-day totals"), "missing per-day section:\n{s}");
        assert!(
            s.contains("Day distribution"),
            "missing day-distribution section:\n{s}"
        );
        assert!(
            s.contains("Most productive day"),
            "missing top-day insight:\n{s}"
        );
        assert!(s.contains("Days active: 1/7"), "missing active-days:\n{s}");
    }

    #[test]
    fn cross_project_insight_fires_when_two_busy_cwds() {
        let lp = pack("english");
        let mut stats = WeekStats {
            total_events: 12,
            ..Default::default()
        };
        let week_start = NaiveDate::from_ymd_opt(2026, 4, 21).unwrap();
        for i in 0..7 {
            stats.days_in_window.push(week_start + Duration::days(i));
            stats.daily_calls.insert(week_start + Duration::days(i), 1);
        }
        stats.cwds.insert("/proj/a".into(), 6);
        stats.cwds.insert("/proj/b".into(), 5);

        let bullets = compute_insights(lp, &stats);
        assert!(
            bullets.iter().any(|b| b.contains("Cross-project")),
            "expected cross-project bullet in {:?}",
            bullets
        );
    }

    #[test]
    fn korean_pack_translates_week_title() {
        let lp = pack("korean");
        let mut stats = WeekStats {
            total_events: 6,
            ..Default::default()
        };
        let week_start = NaiveDate::from_ymd_opt(2026, 4, 21).unwrap();
        for i in 0..7 {
            stats.days_in_window.push(week_start + Duration::days(i));
            stats.daily_calls.insert(week_start + Duration::days(i), 1);
        }
        let mut row = AgentRow::default();
        row.calls = 6;
        stats.agents.insert("claude-code".into(), row);
        let s = render_human(
            lp,
            "Asia/Seoul",
            week_start,
            NaiveDate::from_ymd_opt(2026, 4, 27).unwrap(),
            &stats,
        );
        assert!(s.contains("지난 7일"), "missing ko title:\n{s}");
        assert!(s.contains("일별 합계"), "missing ko per-day heading:\n{s}");
    }
}
