// fluxmirror agent — single-agent filtered report.
//
// Takes a positional `<name>` argument (e.g. `claude-code`,
// `gemini-cli`, `qwen-code`) plus an optional `--period` selector
// (today | yesterday | week, default today). Renders the same body the
// underlying period would produce, but scoped to one agent — the
// per-agent activity table is dropped (only one agent is in scope) and
// the title carries the agent name.
//
// Internally we reuse the existing day/week collectors:
// - today / yesterday → `cmd::report::day::collect_day` with the
//   `agent_filter` parameter set
// - week → a parallel filtered helper that mirrors `collect_week`'s
//   shape; the day/week reports' rendering primitives stay private to
//   this module so the per-agent output diverges cleanly from the
//   per-everyone reports

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::process::ExitCode;

use chrono::{DateTime, Duration, NaiveDate, Utc};
use chrono_tz::Tz;
use rusqlite::Connection;

use crate::cmd::util::{err_exit2, open_db_readonly, parse_tz};
use crate::cmd::window::{day_range, today_range, week_range};
use fluxmirror_core::report::{pack, LangPack};

use super::day::{collect_day, DayStats};
use super::tools::{is_read, is_shell, is_write};
use super::ReportFormat;

/// Maximum rows in the "files written or edited" table.
const FILES_EDITED_LIMIT: usize = 20;
/// Maximum rows in the "files only read" table.
const FILES_READ_LIMIT: usize = 10;
/// Maximum width of the hour bar (used by the today / yesterday paths).
const HOUR_BAR_WIDTH: u32 = 30;
/// Maximum width of the day bar (used by the week path).
const DAY_BAR_WIDTH: u32 = 30;

/// Period selector for the single-agent report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum AgentPeriod {
    Today,
    Yesterday,
    Week,
}

impl Default for AgentPeriod {
    fn default() -> Self {
        AgentPeriod::Today
    }
}

impl std::fmt::Display for AgentPeriod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentPeriod::Today => f.write_str("today"),
            AgentPeriod::Yesterday => f.write_str("yesterday"),
            AgentPeriod::Week => f.write_str("week"),
        }
    }
}

/// CLI args for the agent subcommand.
pub struct AgentArgs {
    pub db: PathBuf,
    pub tz: String,
    pub lang: String,
    pub format: ReportFormat,
    pub agent_name: String,
    pub period: AgentPeriod,
}

pub fn run(args: AgentArgs) -> ExitCode {
    if !matches!(args.format, ReportFormat::Human) {
        // M5 ships --format html for the `week` subcommand only.
        eprintln!(
            "fluxmirror agent: --format {} not yet implemented for this report",
            args.format
        );
        return ExitCode::from(2);
    }
    if args.agent_name.is_empty() {
        eprintln!("fluxmirror agent: <name> argument is required");
        return ExitCode::from(2);
    }

    let tz = match parse_tz(&args.tz) {
        Ok(t) => t,
        Err(e) => return err_exit2(format!("fluxmirror agent: {e}")),
    };
    let conn = match open_db_readonly(&args.db) {
        Ok(c) => c,
        Err(e) => return err_exit2(format!("fluxmirror agent: {e}")),
    };

    let lp = pack(&args.lang);

    let report = match args.period {
        AgentPeriod::Today => render_day_period(
            lp,
            &conn,
            tz,
            &args.tz,
            &args.agent_name,
            args.period,
            today_range(tz),
        ),
        AgentPeriod::Yesterday => render_day_period(
            lp,
            &conn,
            tz,
            &args.tz,
            &args.agent_name,
            args.period,
            day_range(tz, -1),
        ),
        AgentPeriod::Week => render_week_period(lp, &conn, tz, &args.tz, &args.agent_name),
    };

    match report {
        Ok(s) => {
            print!("{}", s);
            ExitCode::SUCCESS
        }
        Err(e) => err_exit2(format!("fluxmirror agent: {e}")),
    }
}

/// Render a today / yesterday report scoped to one agent.
fn render_day_period(
    lp: &LangPack,
    conn: &Connection,
    tz: Tz,
    tz_name: &str,
    agent: &str,
    period: AgentPeriod,
    range: Result<(NaiveDate, DateTime<Utc>, DateTime<Utc>), String>,
) -> Result<String, String> {
    let (date, start, end) = range?;
    let day = collect_day(conn, tz, start, end, Some(agent))?;
    Ok(render_day_human(lp, tz_name, agent, period, date, &day))
}

/// Render a week report scoped to one agent.
fn render_week_period(
    lp: &LangPack,
    conn: &Connection,
    tz: Tz,
    tz_name: &str,
    agent: &str,
) -> Result<String, String> {
    let (week_start, week_end, start, end) = week_range(tz)?;
    let stats = collect_week_for_agent(conn, tz, week_start, agent, start, end)?;
    Ok(render_week_human(
        lp, tz_name, agent, week_start, week_end, &stats,
    ))
}

/// Per-agent week-scoped aggregate. Drops the agents map (only one
/// agent in scope) and skips MCP traffic, but otherwise mirrors the
/// global `WeekStats` struct in `cmd::report::week`.
#[derive(Debug, Default)]
struct AgentWeekStats {
    total_events: u64,
    files_edited: HashMap<(String, String), u64>,
    files_read: HashMap<String, u64>,
    cwds: HashMap<String, u64>,
    tool_mix: HashMap<String, u64>,
    sessions: BTreeSet<String>,
    shell_count: u64,
    days_in_window: Vec<NaiveDate>,
    daily_calls: HashMap<NaiveDate, u64>,
}

fn collect_week_for_agent(
    conn: &Connection,
    tz: Tz,
    week_start: NaiveDate,
    agent: &str,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Result<AgentWeekStats, String> {
    let start_str = start.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let end_str = end.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

    let mut stats = AgentWeekStats::default();
    for i in 0..7 {
        let d = week_start + Duration::days(i);
        stats.days_in_window.push(d);
        stats.daily_calls.insert(d, 0);
    }

    let mut stmt = conn
        .prepare(
            "SELECT ts, COALESCE(session, '') AS session, \
                    COALESCE(tool, '') AS tool, COALESCE(detail, '') AS detail, \
                    COALESCE(cwd, '') AS cwd \
             FROM agent_events \
             WHERE ts >= ?1 AND ts < ?2 AND agent = ?3",
        )
        .map_err(|e| format!("prepare(agent-week): {e}"))?;
    let rows = stmt
        .query_map([&start_str, &end_str, agent], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
            ))
        })
        .map_err(|e| format!("query(agent-week): {e}"))?;

    for row in rows {
        let (ts, session, tool, detail, cwd) = row.map_err(|e| format!("row(agent-week): {e}"))?;
        stats.total_events += 1;

        if !session.is_empty() {
            stats.sessions.insert(session);
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
        } else if is_read(tool_str) && !detail.is_empty() {
            *stats.files_read.entry(detail.clone()).or_default() += 1;
        } else if is_shell(tool_str) {
            stats.shell_count += 1;
        }

        if let Ok(dt) = DateTime::parse_from_rfc3339(&ts) {
            let local_date = dt.with_timezone(&tz).date_naive();
            if let Some(c) = stats.daily_calls.get_mut(&local_date) {
                *c += 1;
            }
        }
    }

    Ok(stats)
}

/// Title prefix for a single-agent report. Caller appends the period
/// suffix and the date(s).
fn format_day_title(
    lp: &LangPack,
    agent: &str,
    period: AgentPeriod,
    date: NaiveDate,
    tz_name: &str,
) -> String {
    let period_label = match period {
        AgentPeriod::Today => lp.today_title,
        AgentPeriod::Yesterday => lp.yesterday_title,
        AgentPeriod::Week => unreachable!("week uses format_week_title"),
    };
    format!(
        "# {agent}: {period_label} ({date}, {tz_name})\n\n",
        agent = agent,
        period_label = period_label,
        date = date.format("%Y-%m-%d"),
        tz_name = tz_name
    )
}

fn format_week_title(
    lp: &LangPack,
    agent: &str,
    start: NaiveDate,
    end: NaiveDate,
    tz_name: &str,
) -> String {
    format!(
        "# {agent}: {title} ({start} ~ {end}, {tz})\n\n",
        agent = agent,
        title = lp.week_title,
        start = start.format("%Y-%m-%d"),
        end = end.format("%Y-%m-%d"),
        tz = tz_name,
    )
}

fn render_day_human(
    lp: &LangPack,
    tz_name: &str,
    agent: &str,
    period: AgentPeriod,
    date: NaiveDate,
    day: &DayStats,
) -> String {
    if day.total_events == 0 {
        // Localized via the today/yesterday no_data lines, prefixed
        // with the agent so the user can see the scope.
        let body = match period {
            AgentPeriod::Today => lp.today_no_data,
            AgentPeriod::Yesterday => lp.yesterday_no_data,
            AgentPeriod::Week => unreachable!(),
        };
        return format!("{agent}: {body}\n");
    }

    let mut out = String::new();
    out.push_str(&format_day_title(lp, agent, period, date, tz_name));

    // Sessions / total calls summary line above the body.
    let sessions: usize = day
        .agents
        .values()
        .map(|a| a.sessions.len())
        .max()
        .unwrap_or(0);
    out.push_str(&format!(
        "Total calls: {} | Sessions: {}\n\n",
        day.total_events, sessions
    ));

    if !day.files_edited.is_empty() {
        out.push_str(&format!("## {}\n\n", lp.today_files_edited_heading));
        let cols = lp.today_columns_file_tool_count;
        out.push_str(&format!("| {} | {} | {} |\n", cols[0], cols[1], cols[2]));
        out.push_str("|---|---|---|\n");
        let mut rows: Vec<(&(String, String), &u64)> = day.files_edited.iter().collect();
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

    if !day.files_read.is_empty() {
        out.push_str(&format!("## {}\n\n", lp.today_files_read_heading));
        let cols = lp.today_columns_path_count;
        out.push_str(&format!("| {} | {} |\n", cols[0], cols[1]));
        out.push_str("|---|---|\n");
        let mut rows: Vec<(&String, &u64)> = day.files_read.iter().collect();
        rows.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
        for (path, n) in rows.iter().take(FILES_READ_LIMIT) {
            out.push_str(&format!("| {} | {} |\n", path, n));
        }
        out.push('\n');
    }

    if !day.shells.is_empty() {
        out.push_str(&format!("## {}\n\n", lp.today_shell_heading));
        let cols = lp.today_columns_time_command;
        out.push_str(&format!("| {} | {} |\n", cols[0], cols[1]));
        out.push_str("|---|---|\n");
        for s in &day.shells {
            let safe = s.detail.replace('|', "\\|");
            out.push_str(&format!("| {} | {} |\n", s.time_local, safe));
        }
        out.push('\n');
    }

    if !day.cwds.is_empty() {
        out.push_str(&format!("## {}\n\n", lp.today_cwds_heading));
        let cols = lp.today_columns_path_count;
        out.push_str(&format!("| {} | {} |\n", cols[0], cols[1]));
        out.push_str("|---|---|\n");
        let mut rows: Vec<(&String, &u64)> = day.cwds.iter().collect();
        rows.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
        for (path, n) in &rows {
            out.push_str(&format!("| {} | {} |\n", path, n));
        }
        out.push('\n');
    }

    if !day.tool_mix.is_empty() {
        out.push_str(&format!("## {}\n\n", lp.today_tool_mix_heading));
        let cols = lp.today_columns_tool_count;
        out.push_str(&format!("| {} | {} |\n", cols[0], cols[1]));
        out.push_str("|---|---|\n");
        let mut rows: Vec<(&String, &u64)> = day.tool_mix.iter().collect();
        rows.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
        for (tool, n) in &rows {
            out.push_str(&format!("| {} | {} |\n", tool, n));
        }
        out.push('\n');
    }

    let max_hour = *day.hours.iter().max().unwrap_or(&0);
    if max_hour > 0 {
        out.push_str(&format!("## {}\n\n", lp.today_hours_heading));
        out.push_str("```\n");
        for (h, n) in day.hours.iter().enumerate() {
            if *n == 0 {
                continue;
            }
            let bar_len = ((*n as u128 * HOUR_BAR_WIDTH as u128) / max_hour as u128) as usize;
            let bar_len = bar_len.max(1);
            let bar: String = std::iter::repeat('█').take(bar_len).collect();
            out.push_str(&format!("{:02}:00 {} {}\n", h, bar, n));
        }
        out.push_str("```\n\n");
    }

    out
}

fn render_week_human(
    lp: &LangPack,
    tz_name: &str,
    agent: &str,
    week_start: NaiveDate,
    week_end: NaiveDate,
    stats: &AgentWeekStats,
) -> String {
    if stats.total_events == 0 {
        return format!("{agent}: {}\n", lp.week_no_data);
    }

    let mut out = String::new();
    out.push_str(&format_week_title(lp, agent, week_start, week_end, tz_name));

    out.push_str(&format!(
        "Total calls: {} | Sessions: {}\n\n",
        stats.total_events,
        stats.sessions.len()
    ));

    // Per-day totals.
    out.push_str(&format!("## {}\n\n", lp.week_daily_totals_heading));
    let cols = lp.week_columns_date_calls;
    out.push_str(&format!("| {} | {} |\n", cols[0], cols[1]));
    out.push_str("|---|---|\n");
    for d in &stats.days_in_window {
        let n = stats.daily_calls.get(d).copied().unwrap_or(0);
        out.push_str(&format!(
            "| {} ({}) | {} |\n",
            d.format("%Y-%m-%d"),
            d.format("%a"),
            n
        ));
    }
    out.push('\n');

    if !stats.files_edited.is_empty() {
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

    if !stats.files_read.is_empty() {
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

    if !stats.tool_mix.is_empty() {
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

    if !stats.cwds.is_empty() {
        out.push_str(&format!("## {}\n\n", lp.today_cwds_heading));
        let cols = lp.today_columns_path_count;
        out.push_str(&format!("| {} | {} |\n", cols[0], cols[1]));
        out.push_str("|---|---|\n");
        let mut rows: Vec<(&String, &u64)> = stats.cwds.iter().collect();
        rows.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
        for (path, n) in &rows {
            out.push_str(&format!("| {} | {} |\n", path, n));
        }
        out.push('\n');
    }

    let max_day = stats.daily_calls.values().copied().max().unwrap_or(0);
    if max_day > 0 {
        out.push_str(&format!("## {}\n\n", lp.week_day_distribution_heading));
        out.push_str("```\n");
        for d in &stats.days_in_window {
            let n = stats.daily_calls.get(d).copied().unwrap_or(0);
            let bar_len = ((n as u128 * DAY_BAR_WIDTH as u128) / max_day as u128) as usize;
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
    fn collect_week_for_agent_filters_to_one_agent() {
        let (_d, path) = fixture_db(&[
            (
                "2026-04-22T10:00:00Z",
                "claude-code",
                "Edit",
                "s1",
                "src/foo.rs",
                "/p",
            ),
            (
                "2026-04-22T11:00:00Z",
                "claude-code",
                "Read",
                "s1",
                "src/bar.rs",
                "/p",
            ),
            (
                "2026-04-23T01:00:00Z",
                "gemini-cli",
                "edit_file",
                "g1",
                "README.md",
                "/q",
            ),
        ]);
        let conn = open_db_readonly(&path).unwrap();
        let tz: Tz = "UTC".parse().unwrap();
        let week_start = NaiveDate::from_ymd_opt(2026, 4, 21).unwrap();
        let start = "2026-04-21T00:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let end = "2026-04-28T00:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let stats =
            collect_week_for_agent(&conn, tz, week_start, "claude-code", start, end).unwrap();
        assert_eq!(stats.total_events, 2);
        assert!(stats.files_edited.contains_key(&("src/foo.rs".into(), "Edit".into())));
        assert!(stats.files_read.contains_key("src/bar.rs"));
        assert!(!stats
            .files_edited
            .contains_key(&("README.md".into(), "edit_file".into())));
    }

    #[test]
    fn render_day_human_skips_other_agents_data() {
        // The DayStats here only contains the requested agent (because
        // collect_day with Some("claude-code") drops the rest), but the
        // render path should never embed any other agent name even
        // accidentally.
        let lp = pack("english");
        let mut day = DayStats::default();
        day.total_events = 3;
        // Pretend collect_day already filtered to claude-code.
        let mut row = super::super::day::AgentRow::default();
        row.calls = 3;
        row.sessions.insert("s1".into());
        day.agents.insert("claude-code".into(), row);
        day.hours[10] = 3;

        let s = render_day_human(
            lp,
            "UTC",
            "claude-code",
            AgentPeriod::Today,
            NaiveDate::from_ymd_opt(2026, 4, 26).unwrap(),
            &day,
        );
        assert!(s.contains("claude-code:"), "missing agent prefix:\n{s}");
        assert!(s.contains("Today's Work"));
        assert!(!s.contains("gemini-cli"));
        assert!(!s.contains("qwen-code"));
    }

    #[test]
    fn render_day_human_no_data_emits_per_agent_no_data_line() {
        let lp = pack("english");
        let day = DayStats::default(); // total_events == 0
        let s = render_day_human(
            lp,
            "UTC",
            "qwen-code",
            AgentPeriod::Yesterday,
            NaiveDate::from_ymd_opt(2026, 4, 25).unwrap(),
            &day,
        );
        assert!(s.starts_with("qwen-code: "), "missing prefix:\n{s}");
        assert!(s.contains("No activity yesterday."), "got:\n{s}");
    }

    #[test]
    fn render_week_human_includes_per_day_and_distribution() {
        let lp = pack("english");
        let mut stats = AgentWeekStats {
            total_events: 5,
            ..Default::default()
        };
        let week_start = NaiveDate::from_ymd_opt(2026, 4, 21).unwrap();
        for i in 0..7 {
            let d = week_start + Duration::days(i);
            stats.days_in_window.push(d);
            stats.daily_calls.insert(d, if i == 1 { 5 } else { 0 });
        }
        stats.sessions.insert("s1".into());

        let s = render_week_human(
            lp,
            "UTC",
            "claude-code",
            week_start,
            NaiveDate::from_ymd_opt(2026, 4, 27).unwrap(),
            &stats,
        );
        assert!(s.contains("claude-code:"));
        assert!(s.contains("Last 7 Days"));
        assert!(s.contains("Per-day totals"));
        assert!(s.contains("Day distribution"));
    }
}
