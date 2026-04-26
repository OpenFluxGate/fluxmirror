// Shared single-day report engine for `today` and `yesterday`.
//
// Pulled out of the original `today.rs` so both day-window reports share
// one aggregation pipeline and one rendering surface. The two slash
// commands then collapse to a thin window-pick + label-pick wrapper.
//
// Sections rendered, in order:
//   1. Title (per language) + date header
//   2. Activity stats (Agent | Calls | Sessions)
//   3. Files written or edited (top 20)
//   4. Files only read (top 10)
//   5. Shell commands (all rows, ordered by ts)
//   6. Working directories
//   7. MCP traffic methods (skipped if empty)
//   8. Tool mix
//   9. Hour distribution (text bar chart, 30-char max width)
//   10. Insights — three deterministic bullets max:
//       - busiest hour, always emitted when ≥1 row
//       - edit-to-read ratio, always emitted when reads > 0
//       - multi-project rule fires only when ≥2 cwds have ≥5 calls
//
// Empty data (< 5 rows total): emit the localised "limited activity"
// line and exit 0.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use chrono::{DateTime, NaiveDate, Timelike, Utc};
use chrono_tz::Tz;
use rusqlite::Connection;

use fluxmirror_core::report::LangPack;

use super::tools::{is_read, is_shell, is_write};

/// Maximum rows in the "files written or edited" table.
pub(crate) const FILES_EDITED_LIMIT: usize = 20;
/// Maximum rows in the "files only read" table.
pub(crate) const FILES_READ_LIMIT: usize = 10;
/// Maximum width (chars) of the bar in the hour-distribution chart.
const HOUR_BAR_WIDTH: u32 = 30;
/// Truncation length for shell-command detail strings.
const SHELL_DETAIL_MAX_CHARS: usize = 80;
/// Below this number of total events, emit the "limited activity" line
/// and exit 0.
pub(crate) const LIMITED_ACTIVITY_THRESHOLD: u64 = 5;
/// A working directory needs at least this many calls to count toward
/// the "multi-project day" insight.
const MULTI_PROJECT_CWD_MIN_CALLS: u64 = 5;

/// Single shell-command event used in the "Shell commands" table.
#[derive(Debug, Clone)]
pub(crate) struct ShellRow {
    pub time_local: String, // HH:MM in user TZ
    pub detail: String,     // truncated to SHELL_DETAIL_MAX_CHARS
    pub ts_utc: DateTime<Utc>,
}

/// All aggregates for a single day's window. Computed once,
/// rendered repeatedly.
#[derive(Debug, Default)]
pub(crate) struct DayStats {
    /// Total events in the window (drives the empty-data branch).
    pub total_events: u64,
    /// Per-agent call / session counts. BTreeMap for stable order
    /// before the desc-by-calls sort.
    pub agents: BTreeMap<String, AgentRow>,
    /// (path, tool) -> count, for write-class events with non-empty
    /// detail. Sorted desc by count at render time.
    pub files_edited: HashMap<(String, String), u64>,
    /// path -> count, for read-class events with non-empty detail.
    pub files_read: HashMap<String, u64>,
    /// Every shell-class row in chronological order.
    pub shells: Vec<ShellRow>,
    /// cwd -> count.
    pub cwds: HashMap<String, u64>,
    /// MCP method -> count, from the `events` table.
    pub mcp_methods: HashMap<String, u64>,
    /// raw tool name -> count.
    pub tool_mix: HashMap<String, u64>,
    /// Hour-of-day buckets (0..=23) in the user's TZ.
    pub hours: [u64; 24],
    /// Cached read-class total — the edit-to-read ratio insight needs
    /// it and recomputing per-render would be redundant.
    pub reads_total: u64,
    /// Cached write-class total — same reason.
    pub writes_total: u64,
    /// Distinct file paths touched by either a write or a read with
    /// non-empty detail. Used by `compare` for the diff column; today
    /// and yesterday don't surface it directly.
    pub distinct_files: BTreeSet<String>,
}

/// Per-agent row in the activity stats table.
#[derive(Debug, Default, Clone)]
pub(crate) struct AgentRow {
    pub calls: u64,
    pub sessions: BTreeSet<String>,
}

/// Run the whole day-aggregation pipeline against the connection.
///
/// Pulls every `agent_events` row in the window plus the matching
/// `events` rows for MCP traffic and folds them into one `DayStats`.
/// Each loop classifies the row exactly once and updates the relevant
/// maps in-place — single pass over the data.
///
/// `agent_filter`, when `Some(name)`, restricts the aggregation to rows
/// where `agent = ?name`. Used by the `agent` subcommand to scope the
/// same report to a single agent without a separate code path.
pub(crate) fn collect_day(
    conn: &Connection,
    tz: Tz,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    agent_filter: Option<&str>,
) -> Result<DayStats, String> {
    let start_str = start.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let end_str = end.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let start_ms = start.timestamp_millis();
    let end_ms = end.timestamp_millis();

    let mut day = DayStats::default();

    // Pass 1 — every agent_events row in the window. We collect the
    // (ts, agent, session, tool, detail, cwd) tuples upfront so the
    // optional agent filter fork only affects the SQL prepare/query
    // pair; the row-handling loop below stays branchless. Pulling all
    // matching rows for a single day's worth of events is fine — even
    // a heavy day stays well under 10k rows.
    let row_t = |r: &rusqlite::Row<'_>| {
        Ok((
            r.get::<_, String>(0)?, // ts
            r.get::<_, String>(1)?, // agent
            r.get::<_, String>(2)?, // session
            r.get::<_, String>(3)?, // tool
            r.get::<_, String>(4)?, // detail
            r.get::<_, String>(5)?, // cwd
        ))
    };
    let collected: Vec<(String, String, String, String, String, String)> = match agent_filter {
        Some(agent) => {
            let mut stmt = conn
                .prepare(
                    "SELECT ts, agent, COALESCE(session, '') AS session, \
                            COALESCE(tool, '') AS tool, COALESCE(detail, '') AS detail, \
                            COALESCE(cwd, '') AS cwd \
                     FROM agent_events WHERE ts >= ?1 AND ts < ?2 AND agent = ?3",
                )
                .map_err(|e| format!("prepare(events): {e}"))?;
            let mapped = stmt
                .query_map([&start_str, &end_str, agent], row_t)
                .map_err(|e| format!("query(events): {e}"))?;
            let mut out = Vec::new();
            for row in mapped {
                out.push(row.map_err(|e| format!("row(events): {e}"))?);
            }
            out
        }
        None => {
            let mut stmt = conn
                .prepare(
                    "SELECT ts, agent, COALESCE(session, '') AS session, \
                            COALESCE(tool, '') AS tool, COALESCE(detail, '') AS detail, \
                            COALESCE(cwd, '') AS cwd \
                     FROM agent_events WHERE ts >= ?1 AND ts < ?2",
                )
                .map_err(|e| format!("prepare(events): {e}"))?;
            let mapped = stmt
                .query_map([&start_str, &end_str], row_t)
                .map_err(|e| format!("query(events): {e}"))?;
            let mut out = Vec::new();
            for row in mapped {
                out.push(row.map_err(|e| format!("row(events): {e}"))?);
            }
            out
        }
    };

    for (ts, agent, session, tool, detail, cwd) in collected {

        day.total_events += 1;

        // Agent stats.
        let entry = day.agents.entry(agent).or_default();
        entry.calls += 1;
        if !session.is_empty() {
            entry.sessions.insert(session);
        }

        // Tool mix.
        if !tool.is_empty() {
            *day.tool_mix.entry(tool.clone()).or_default() += 1;
        }

        // CWD distribution.
        if !cwd.is_empty() {
            *day.cwds.entry(cwd).or_default() += 1;
        }

        // Files / shell classification — write-class with detail goes
        // into files_edited; read-class with detail goes into files_read;
        // shell-class regardless of detail goes into the shell table.
        let tool_str = tool.as_str();
        if is_write(tool_str) && !detail.is_empty() {
            *day
                .files_edited
                .entry((detail.clone(), tool.clone()))
                .or_default() += 1;
            day.writes_total += 1;
            day.distinct_files.insert(detail.clone());
        } else if is_read(tool_str) && !detail.is_empty() {
            *day.files_read.entry(detail.clone()).or_default() += 1;
            day.reads_total += 1;
            day.distinct_files.insert(detail.clone());
        } else if is_shell(tool_str) {
            // Shell rows: keep ts (for chronological order), the local
            // HH:MM rendering, and a truncated detail.
            if let Ok(dt) = DateTime::parse_from_rfc3339(&ts) {
                let dt_utc = dt.with_timezone(&Utc);
                let local = dt_utc.with_timezone(&tz);
                let time_local = format!("{:02}:{:02}", local.hour(), local.minute());
                day.shells.push(ShellRow {
                    time_local,
                    detail: truncate_chars(&detail, SHELL_DETAIL_MAX_CHARS),
                    ts_utc: dt_utc,
                });
            }
        } else if is_write(tool_str) {
            // Write-class but empty detail: still counts toward the
            // edit-to-read ratio.
            day.writes_total += 1;
        } else if is_read(tool_str) {
            day.reads_total += 1;
        }

        // Hour-of-day bucket. Mirrors the histogram subcommand: parse
        // the RFC 3339 timestamp, convert to the user TZ, bucket on
        // local hour. Rows that fail to parse are silently skipped to
        // match histogram's behaviour.
        if let Ok(dt) = DateTime::parse_from_rfc3339(&ts) {
            let local = dt.with_timezone(&tz);
            let h = local.hour() as usize;
            day.hours[h] = day.hours[h].saturating_add(1);
        }
    }

    // Sort shell rows chronologically.
    day.shells.sort_by_key(|s| s.ts_utc);

    // Pass 2 — MCP traffic from the `events` table. Best-effort: a
    // missing `events` table on a legacy DB shouldn't kill the report.
    // We only run this when no agent filter is in effect — the events
    // table doesn't carry an agent column so filtering it would be
    // ambiguous, and the agent subcommand explicitly scopes to one
    // agent's tool calls only.
    if agent_filter.is_none() {
        if let Ok(mut stmt) = conn.prepare(
            "SELECT method FROM events \
             WHERE ts_ms >= ?1 AND ts_ms < ?2 AND method IS NOT NULL",
        ) {
            let rows = stmt.query_map([&start_ms, &end_ms], |r| r.get::<_, String>(0));
            if let Ok(rows) = rows {
                for row in rows.flatten() {
                    if !row.is_empty() {
                        *day.mcp_methods.entry(row).or_default() += 1;
                    }
                }
            }
        }
    }

    Ok(day)
}

/// Truncate a string to at most `max` Unicode characters, appending
/// nothing — the caller can decide whether to add an ellipsis. We use
/// char-count rather than byte-count so multi-byte code points (CJK,
/// emoji) don't get sliced mid-codepoint.
fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    s.chars().take(max).collect()
}

/// Heading style picked by the caller. `today` and `yesterday` use the
/// same body but different localized title strings; this enum lets the
/// shared renderer pick the right `LangPack` field without growing a
/// boolean parameter.
#[derive(Copy, Clone)]
pub(crate) enum DayLabel {
    Today,
    Yesterday,
}

impl DayLabel {
    fn title<'a>(self, lp: &'a LangPack) -> &'a str {
        match self {
            DayLabel::Today => lp.today_title,
            DayLabel::Yesterday => lp.yesterday_title,
        }
    }
    fn no_data<'a>(self, lp: &'a LangPack) -> &'a str {
        match self {
            DayLabel::Today => lp.today_no_data,
            DayLabel::Yesterday => lp.yesterday_no_data,
        }
    }
}

/// Render the finished human-readable report. Trailing newline included.
pub(crate) fn render_human(
    lp: &LangPack,
    tz_name: &str,
    date: NaiveDate,
    day: &DayStats,
    label: DayLabel,
) -> String {
    if day.total_events < LIMITED_ACTIVITY_THRESHOLD {
        return format!("{}\n", label.no_data(lp));
    }

    let mut out = String::new();

    // Title: "# <Title> (YYYY-MM-DD <tz>)".
    out.push_str(&format!(
        "# {} ({}, {})\n\n",
        label.title(lp),
        date.format("%Y-%m-%d"),
        tz_name
    ));

    render_activity(&mut out, lp, day);
    render_files_edited(&mut out, lp, day);
    render_files_read(&mut out, lp, day);
    render_shells(&mut out, lp, day);
    render_cwds(&mut out, lp, day);
    render_mcp(&mut out, lp, day);
    render_tool_mix(&mut out, lp, day);
    render_hours(&mut out, lp, day);
    render_insights(&mut out, lp, day);

    out
}

fn render_activity(out: &mut String, lp: &LangPack, day: &DayStats) {
    out.push_str(&format!("## {}\n\n", lp.today_activity_heading));
    let cols = lp.today_columns_calls_sessions;
    out.push_str(&format!("| {} | {} | {} |\n", cols[0], cols[1], cols[2]));
    out.push_str("|---|---|---|\n");

    let mut rows: Vec<(&String, &AgentRow)> = day.agents.iter().collect();
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

fn render_files_edited(out: &mut String, lp: &LangPack, day: &DayStats) {
    if day.files_edited.is_empty() {
        return;
    }
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

fn render_files_read(out: &mut String, lp: &LangPack, day: &DayStats) {
    if day.files_read.is_empty() {
        return;
    }
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

fn render_shells(out: &mut String, lp: &LangPack, day: &DayStats) {
    if day.shells.is_empty() {
        return;
    }
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

fn render_cwds(out: &mut String, lp: &LangPack, day: &DayStats) {
    if day.cwds.is_empty() {
        return;
    }
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

fn render_mcp(out: &mut String, lp: &LangPack, day: &DayStats) {
    if day.mcp_methods.is_empty() {
        return;
    }
    out.push_str(&format!("## {}\n\n", lp.today_mcp_heading));
    let cols = lp.today_columns_method_count;
    out.push_str(&format!("| {} | {} |\n", cols[0], cols[1]));
    out.push_str("|---|---|\n");

    let mut rows: Vec<(&String, &u64)> = day.mcp_methods.iter().collect();
    rows.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
    for (method, n) in &rows {
        out.push_str(&format!("| {} | {} |\n", method, n));
    }
    out.push('\n');
}

fn render_tool_mix(out: &mut String, lp: &LangPack, day: &DayStats) {
    if day.tool_mix.is_empty() {
        return;
    }
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

fn render_hours(out: &mut String, lp: &LangPack, day: &DayStats) {
    let max = *day.hours.iter().max().unwrap_or(&0);
    if max == 0 {
        return;
    }
    out.push_str(&format!("## {}\n\n", lp.today_hours_heading));
    out.push_str("```\n");
    for (h, n) in day.hours.iter().enumerate() {
        if *n == 0 {
            continue;
        }
        let bar_len = ((*n as u128 * HOUR_BAR_WIDTH as u128) / max as u128) as usize;
        // Always show at least one bar char for non-zero rows so a
        // single outlier doesn't render as a blank line.
        let bar_len = bar_len.max(1);
        let bar: String = std::iter::repeat('█').take(bar_len).collect();
        out.push_str(&format!("{:02}:00 {} {}\n", h, bar, n));
    }
    out.push_str("```\n\n");
}

fn render_insights(out: &mut String, lp: &LangPack, day: &DayStats) {
    let bullets = compute_insights(lp, day);
    if bullets.is_empty() {
        return;
    }
    out.push_str(&format!("## {}\n\n", lp.today_insights_heading));
    for b in &bullets {
        out.push_str(&format!("- {}\n", b));
    }
}

/// Three-bullet insight surface, deterministic. No model inference.
///
/// Rules (in order, capped at 3 bullets total):
/// 1. Most productive hour — always emitted when ≥1 hour has events.
/// 2. Edit-to-read ratio — emitted when reads_total > 0.
/// 3. Multi-project day — emitted when ≥2 cwds each have ≥5 calls.
pub(crate) fn compute_insights(lp: &LangPack, day: &DayStats) -> Vec<String> {
    let mut bullets: Vec<String> = Vec::new();

    if let Some((h, n)) = busiest_hour(&day.hours) {
        bullets.push(
            lp.today_insight_busiest_hour
                .replace("{hour}", &format!("{:02}:00", h))
                .replace("{n}", &n.to_string()),
        );
    }

    if day.reads_total > 0 {
        let ratio = day.writes_total as f64 / day.reads_total as f64;
        bullets.push(
            lp.today_insight_edit_read_ratio
                .replace("{ratio}", &format!("{:.2}", ratio)),
        );
    }

    let busy_cwds = day
        .cwds
        .values()
        .filter(|n| **n >= MULTI_PROJECT_CWD_MIN_CALLS)
        .count();
    if busy_cwds >= 2 {
        bullets.push(
            lp.today_insight_multi_project
                .replace("{n}", &busy_cwds.to_string()),
        );
    }

    bullets
}

/// Hour with the highest count in `hours`. Ties resolve to the
/// earliest hour (smallest index) for determinism.
pub(crate) fn busiest_hour(hours: &[u64; 24]) -> Option<(usize, u64)> {
    let mut best: Option<(usize, u64)> = None;
    for (i, n) in hours.iter().enumerate() {
        if *n == 0 {
            continue;
        }
        match best {
            None => best = Some((i, *n)),
            Some((_, prev)) if *n > prev => best = Some((i, *n)),
            _ => {}
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmd::util::open_db_readonly;
    use fluxmirror_core::report::pack;
    use fluxmirror_store::SqliteStore;
    use rusqlite::params;
    use std::path::PathBuf;
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
    fn truncate_chars_handles_cjk() {
        let s = "한국어 테스트 문자열입니다 abcdef";
        let t = truncate_chars(s, 5);
        assert_eq!(t.chars().count(), 5);
    }

    #[test]
    fn truncate_chars_passthrough_short() {
        assert_eq!(truncate_chars("abc", 80), "abc");
    }

    #[test]
    fn busiest_hour_returns_max_with_earliest_tiebreak() {
        let mut h = [0u64; 24];
        h[2] = 5;
        h[10] = 5;
        let (i, n) = busiest_hour(&h).unwrap();
        assert_eq!(i, 2);
        assert_eq!(n, 5);
    }

    #[test]
    fn busiest_hour_empty_returns_none() {
        let h = [0u64; 24];
        assert!(busiest_hour(&h).is_none());
    }

    #[test]
    fn collect_day_aggregates_basic_counts() {
        let (_d, path) = fixture_db(&[
            (
                "2026-04-26T01:00:00Z",
                "claude-code",
                "Edit",
                "s1",
                "src/foo.rs",
                "/proj/a",
            ),
            (
                "2026-04-26T02:00:00Z",
                "claude-code",
                "Read",
                "s1",
                "src/bar.rs",
                "/proj/a",
            ),
            (
                "2026-04-26T03:00:00Z",
                "gemini-cli",
                "Bash",
                "g1",
                "cargo test",
                "/proj/b",
            ),
        ]);
        let conn = open_db_readonly(&path).unwrap();
        let tz: Tz = "UTC".parse().unwrap();
        let start = "2026-04-26T00:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let end = "2026-04-27T00:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let day = collect_day(&conn, tz, start, end, None).unwrap();

        assert_eq!(day.total_events, 3);
        assert_eq!(day.agents.get("claude-code").unwrap().calls, 2);
        assert_eq!(day.agents.get("gemini-cli").unwrap().calls, 1);
        assert_eq!(day.writes_total, 1);
        assert_eq!(day.reads_total, 1);
        assert_eq!(day.shells.len(), 1);
        assert_eq!(day.cwds.get("/proj/a").copied(), Some(2));
        assert_eq!(day.cwds.get("/proj/b").copied(), Some(1));
        assert!(day.distinct_files.contains("src/foo.rs"));
        assert!(day.distinct_files.contains("src/bar.rs"));
    }

    #[test]
    fn collect_day_filters_to_one_agent_when_requested() {
        let (_d, path) = fixture_db(&[
            (
                "2026-04-26T01:00:00Z",
                "claude-code",
                "Edit",
                "s1",
                "src/foo.rs",
                "/proj/a",
            ),
            (
                "2026-04-26T02:00:00Z",
                "gemini-cli",
                "read_file",
                "g1",
                "src/bar.rs",
                "/proj/b",
            ),
        ]);
        let conn = open_db_readonly(&path).unwrap();
        let tz: Tz = "UTC".parse().unwrap();
        let start = "2026-04-26T00:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let end = "2026-04-27T00:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let day = collect_day(&conn, tz, start, end, Some("claude-code")).unwrap();

        assert_eq!(day.total_events, 1);
        assert!(day.agents.contains_key("claude-code"));
        assert!(!day.agents.contains_key("gemini-cli"));
    }

    #[test]
    fn render_human_short_window_emits_no_data_line() {
        let lp = pack("english");
        let day = DayStats {
            total_events: 2,
            ..Default::default()
        };
        let s = render_human(
            lp,
            "UTC",
            NaiveDate::from_ymd_opt(2026, 4, 26).unwrap(),
            &day,
            DayLabel::Today,
        );
        assert!(s.contains("Limited activity today."), "got:\n{s}");
    }

    #[test]
    fn render_human_yesterday_label_emits_yesterday_no_data() {
        let lp = pack("english");
        let day = DayStats {
            total_events: 0,
            ..Default::default()
        };
        let s = render_human(
            lp,
            "UTC",
            NaiveDate::from_ymd_opt(2026, 4, 25).unwrap(),
            &day,
            DayLabel::Yesterday,
        );
        assert!(s.contains("No activity yesterday."), "got:\n{s}");
    }

    #[test]
    fn render_human_includes_title_and_activity_table() {
        let lp = pack("english");
        let mut day = DayStats {
            total_events: 6,
            ..Default::default()
        };
        let mut row = AgentRow::default();
        row.calls = 6;
        row.sessions.insert("s1".into());
        day.agents.insert("claude-code".into(), row);
        day.hours[10] = 6;
        day.cwds.insert("/proj/a".into(), 6);

        let s = render_human(
            lp,
            "Asia/Seoul",
            NaiveDate::from_ymd_opt(2026, 4, 26).unwrap(),
            &day,
            DayLabel::Today,
        );
        assert!(s.contains("Today's Work"), "missing title:\n{s}");
        assert!(s.contains("2026-04-26"), "missing date:\n{s}");
        assert!(s.contains("Asia/Seoul"), "missing tz:\n{s}");
        assert!(s.contains("claude-code"), "missing agent row:\n{s}");
        assert!(
            s.contains("Most productive hour"),
            "missing busiest hour:\n{s}"
        );
    }

    #[test]
    fn render_human_yesterday_uses_yesterday_title() {
        let lp = pack("english");
        let mut day = DayStats {
            total_events: 6,
            ..Default::default()
        };
        let mut row = AgentRow::default();
        row.calls = 6;
        row.sessions.insert("s1".into());
        day.agents.insert("claude-code".into(), row);
        day.hours[10] = 6;
        day.cwds.insert("/proj/a".into(), 6);

        let s = render_human(
            lp,
            "Asia/Seoul",
            NaiveDate::from_ymd_opt(2026, 4, 25).unwrap(),
            &day,
            DayLabel::Yesterday,
        );
        assert!(s.contains("Yesterday's Work"), "missing yesterday title:\n{s}");
    }

    #[test]
    fn compute_insights_emits_busiest_and_ratio_when_present() {
        let lp = pack("english");
        let mut day = DayStats::default();
        day.hours[11] = 10;
        day.writes_total = 4;
        day.reads_total = 5; // ratio 0.80
        let bullets = compute_insights(lp, &day);
        assert_eq!(bullets.len(), 2);
        assert!(bullets[0].contains("11:00"));
        assert!(bullets[1].contains("0.80"), "got {:?}", bullets[1]);
    }

    #[test]
    fn compute_insights_multi_project_rule_fires_with_two_busy_cwds() {
        let lp = pack("english");
        let mut day = DayStats::default();
        day.hours[9] = 1;
        day.cwds.insert("/proj/a".into(), 6);
        day.cwds.insert("/proj/b".into(), 5);
        day.cwds.insert("/proj/c".into(), 1);
        let bullets = compute_insights(lp, &day);
        assert!(
            bullets.iter().any(|b| b.contains("Multi-project")),
            "expected multi-project bullet in {:?}",
            bullets
        );
    }

    #[test]
    fn shell_table_truncates_long_detail() {
        let long_cmd = "x".repeat(200);
        let (_d, path) = fixture_db(&[
            ("2026-04-26T01:00:00Z", "claude-code", "Bash", "s1", &long_cmd, "/p"),
            ("2026-04-26T02:00:00Z", "claude-code", "Bash", "s1", "echo hi", "/p"),
            ("2026-04-26T03:00:00Z", "claude-code", "Bash", "s1", "echo hi", "/p"),
            ("2026-04-26T04:00:00Z", "claude-code", "Bash", "s1", "echo hi", "/p"),
            ("2026-04-26T05:00:00Z", "claude-code", "Bash", "s1", "echo hi", "/p"),
        ]);
        let conn = open_db_readonly(&path).unwrap();
        let tz: Tz = "UTC".parse().unwrap();
        let start = "2026-04-26T00:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let end = "2026-04-27T00:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let day = collect_day(&conn, tz, start, end, None).unwrap();
        let lp = pack("english");
        let s = render_human(
            lp,
            "UTC",
            NaiveDate::from_ymd_opt(2026, 4, 26).unwrap(),
            &day,
            DayLabel::Today,
        );
        assert!(!s.contains(&"x".repeat(SHELL_DETAIL_MAX_CHARS + 1)));
        assert!(s.contains("Shell commands"));
    }
}
