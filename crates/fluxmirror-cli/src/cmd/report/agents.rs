// fluxmirror agents — per-agent quick stats for the past 7 days.
//
// First vertical slice of M1: replaces the markdown-driven slash command
// (which embedded shell + SQL and asked the model to render the result)
// with a single Rust subcommand that emits the finished report on
// stdout. The slash command files shrink to a 2-line wrapper that calls
// this binary and forwards its output verbatim.
//
// Aggregations match the v0.5.x agents.md command surface:
//   1. per-agent calls / sessions / first-ts / last-ts
//   2. per-agent dominant tool (the most-frequent `tool` value)
//   3. per-agent write/read/shell tool-class breakdown
//   4. per-agent active days (distinct local-date buckets)
//
// Insights are rule-based — no model inference. Three rules:
//   - the agent with the highest call count
//   - any agent whose entire window is a single session
//   - any agent whose write share exceeds 70% of its total calls

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;
use std::process::ExitCode;

use chrono::{DateTime, NaiveDate, Utc};
use chrono_tz::Tz;
use rusqlite::Connection;

use crate::cmd::util::{err_exit2, open_db_readonly, parse_tz};
use crate::cmd::window::week_range;
use fluxmirror_core::report::{pack, LangPack};

use super::ReportFormat;

/// Tool names that count as "writes" for the share-of-calls breakdown.
/// Mirrors the legacy slash command surface (Edit, Write, MultiEdit,
/// plus the gemini/qwen camelCase variants).
const WRITE_TOOLS: &[&str] = &[
    "Edit",
    "Write",
    "MultiEdit",
    "edit_file",
    "write_file",
    "replace",
];
const READ_TOOLS: &[&str] = &["Read", "read_file", "read_many_files"];
const SHELL_TOOLS: &[&str] = &["Bash", "run_shell_command"];

/// Threshold (write-share, 0..=100) above which the write-heavy
/// insight rule fires.
const WRITE_HEAVY_PCT: u32 = 70;

/// Args for `cmd::report::agents::run`. The clap layer in `main.rs`
/// builds one of these from its own clap-derive struct so this module
/// stays free of clap types and tests can call `run()` directly.
pub struct AgentsArgs {
    /// SQLite events database (read-only).
    pub db: PathBuf,
    /// IANA timezone for the 7-day window calculation.
    pub tz: String,
    /// Language code (canonical: english | korean | japanese | chinese).
    pub lang: String,
    /// Output format. Only `Human` is implemented in M1.
    pub format: ReportFormat,
}

pub fn run(args: AgentsArgs) -> ExitCode {
    if !matches!(args.format, ReportFormat::Human) {
        eprintln!(
            "fluxmirror agents: --format {} not yet implemented (M1 ships --format human only)",
            args.format
        );
        return ExitCode::from(2);
    }

    let tz = match parse_tz(&args.tz) {
        Ok(t) => t,
        Err(e) => return err_exit2(format!("fluxmirror agents: {e}")),
    };
    let (week_start, week_end, start_utc, end_utc) = match week_range(tz) {
        Ok(r) => r,
        Err(e) => return err_exit2(format!("fluxmirror agents: {e}")),
    };
    let conn = match open_db_readonly(&args.db) {
        Ok(c) => c,
        Err(e) => return err_exit2(format!("fluxmirror agents: {e}")),
    };

    let stats = match collect_stats(&conn, tz, start_utc, end_utc) {
        Ok(s) => s,
        Err(e) => return err_exit2(format!("fluxmirror agents: {e}")),
    };

    let lp = pack(&args.lang);
    let report = render_human(lp, &args.tz, week_start, week_end, &stats);
    print!("{}", report);
    ExitCode::SUCCESS
}

/// Per-agent aggregate row used for both rendering and the rule-based
/// insights. `BTreeMap` keeps agents alphabetically ordered for stable
/// snapshots; the rendered table is then re-sorted by `calls` desc.
#[derive(Debug, Default, Clone)]
pub(crate) struct AgentStat {
    pub calls: u64,
    pub sessions: u64,
    pub active_days: u64,
    pub dominant_tool: String,
    pub writes: u64,
    pub reads: u64,
    pub shells: u64,
}

/// Run the four aggregations against the connection and fold them into
/// one row per agent. The query sequence mirrors the legacy slash
/// command (one pass per metric) — each pass is cheap enough at this
/// scale that we don't bother fusing them.
pub(crate) fn collect_stats(
    conn: &Connection,
    tz: Tz,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Result<BTreeMap<String, AgentStat>, String> {
    let start_str = start.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let end_str = end.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

    let mut stats: BTreeMap<String, AgentStat> = BTreeMap::new();

    // Pass 1: calls + distinct sessions per agent.
    let mut stmt = conn
        .prepare(
            "SELECT agent, COUNT(*) AS calls, COUNT(DISTINCT session) AS sessions \
             FROM agent_events WHERE ts >= ?1 AND ts < ?2 GROUP BY agent",
        )
        .map_err(|e| format!("prepare(calls): {e}"))?;
    let rows = stmt
        .query_map([&start_str, &end_str], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)? as u64,
                r.get::<_, i64>(2)? as u64,
            ))
        })
        .map_err(|e| format!("query(calls): {e}"))?;
    for row in rows {
        let (agent, calls, sessions) = row.map_err(|e| format!("row(calls): {e}"))?;
        let entry = stats.entry(agent).or_default();
        entry.calls = calls;
        entry.sessions = sessions;
    }
    drop(stmt);

    // Pass 2: dominant tool per agent. We pull every (agent, tool, n)
    // triple ordered by n desc and take the first hit per agent.
    let mut stmt = conn
        .prepare(
            "SELECT agent, COALESCE(tool, '') AS tool, COUNT(*) AS n \
             FROM agent_events WHERE ts >= ?1 AND ts < ?2 \
             GROUP BY agent, tool ORDER BY n DESC",
        )
        .map_err(|e| format!("prepare(dominant): {e}"))?;
    let rows = stmt
        .query_map([&start_str, &end_str], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)? as u64,
            ))
        })
        .map_err(|e| format!("query(dominant): {e}"))?;
    for row in rows {
        let (agent, tool, _n) = row.map_err(|e| format!("row(dominant): {e}"))?;
        let entry = stats.entry(agent).or_default();
        if entry.dominant_tool.is_empty() {
            entry.dominant_tool = tool;
        }
    }
    drop(stmt);

    // Pass 3: write / read / shell breakdown. We tally in Rust so the
    // tool-class lists stay in one place and the SQL stays a plain
    // SELECT (no IN-list interpolation).
    let mut stmt = conn
        .prepare(
            "SELECT agent, COALESCE(tool, '') AS tool, COUNT(*) AS n \
             FROM agent_events WHERE ts >= ?1 AND ts < ?2 GROUP BY agent, tool",
        )
        .map_err(|e| format!("prepare(class): {e}"))?;
    let rows = stmt
        .query_map([&start_str, &end_str], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)? as u64,
            ))
        })
        .map_err(|e| format!("query(class): {e}"))?;
    for row in rows {
        let (agent, tool, n) = row.map_err(|e| format!("row(class): {e}"))?;
        let entry = stats.entry(agent).or_default();
        if WRITE_TOOLS.contains(&tool.as_str()) {
            entry.writes += n;
        } else if READ_TOOLS.contains(&tool.as_str()) {
            entry.reads += n;
        } else if SHELL_TOOLS.contains(&tool.as_str()) {
            entry.shells += n;
        }
    }
    drop(stmt);

    // Pass 4: active days. We bucket in Rust because SQLite's
    // `substr(ts, 1, 10)` would split on UTC boundaries; reports are
    // always anchored to the user's local TZ.
    let mut per_agent_days: HashMap<String, BTreeSet<NaiveDate>> = HashMap::new();
    let mut stmt = conn
        .prepare(
            "SELECT agent, ts FROM agent_events \
             WHERE ts >= ?1 AND ts < ?2",
        )
        .map_err(|e| format!("prepare(days): {e}"))?;
    let rows = stmt
        .query_map([&start_str, &end_str], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })
        .map_err(|e| format!("query(days): {e}"))?;
    for row in rows {
        let (agent, ts) = row.map_err(|e| format!("row(days): {e}"))?;
        if let Ok(dt) = DateTime::parse_from_rfc3339(&ts) {
            let local = dt.with_timezone(&tz).date_naive();
            per_agent_days
                .entry(agent)
                .or_insert_with(BTreeSet::new)
                .insert(local);
        }
    }
    drop(stmt);
    for (agent, days) in per_agent_days {
        let entry = stats.entry(agent).or_default();
        entry.active_days = days.len() as u64;
    }

    Ok(stats)
}

/// Render the finished human-readable report. Trailing newline included.
fn render_human(
    lp: &LangPack,
    tz_name: &str,
    week_start: NaiveDate,
    week_end: NaiveDate,
    stats: &BTreeMap<String, AgentStat>,
) -> String {
    if stats.is_empty() {
        return format!("{}\n", lp.agents_no_activity);
    }

    let mut out = String::new();
    // Title line: "# Agent Roster (last 7 days, Asia/Seoul)"
    out.push_str(&format!(
        "# {} ({}, {})\n",
        lp.agents_title, lp.tz_label, tz_name
    ));
    // Range line: "Range: 2026-04-21 .. 2026-04-27"
    out.push_str(&format!(
        "{}: {} .. {}\n\n",
        lp.range_label,
        week_start.format("%Y-%m-%d"),
        week_end.format("%Y-%m-%d")
    ));

    // Markdown table header.
    out.push_str(&format!(
        "| {} | {} | {} | {} | {} | {} |\n",
        lp.agents_columns[0],
        lp.agents_columns[1],
        lp.agents_columns[2],
        lp.agents_columns[3],
        lp.agents_columns[4],
        lp.agents_columns[5],
    ));
    out.push_str("|---|---|---|---|---|---|\n");

    // Sort by calls desc; tie-break on agent name asc for stability.
    let mut rows: Vec<(&String, &AgentStat)> = stats.iter().collect();
    rows.sort_by(|a, b| b.1.calls.cmp(&a.1.calls).then_with(|| a.0.cmp(b.0)));

    for (agent, s) in &rows {
        let dom = if s.dominant_tool.is_empty() {
            "-".to_string()
        } else {
            s.dominant_tool.clone()
        };
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {}/{}/{} |\n",
            agent,
            s.calls,
            s.sessions,
            s.active_days,
            dom,
            s.writes,
            s.reads,
            s.shells,
        ));
    }

    // Insights section — at most 3 bullets, deterministic order.
    let bullets = compute_insights(lp, &rows);
    if !bullets.is_empty() {
        out.push_str(&format!("\n## {}\n", lp.insights_heading));
        for b in &bullets {
            out.push_str(&format!("- {}\n", b));
        }
    }

    out
}

/// Compute up to three insight bullets from the sorted-rows view.
///
/// Rules (in order):
/// 1. Busiest agent — top of the desc-sorted list. Always emitted.
/// 2. One-shot agents — `sessions == 1`. Emitted once per matching
///    agent so a friend who only ran a single qwen session sees it.
/// 3. Write-heavy agents — `writes / calls > WRITE_HEAVY_PCT`.
fn compute_insights(lp: &LangPack, rows: &[(&String, &AgentStat)]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();

    if let Some((agent, s)) = rows.first() {
        out.push(
            lp.insight_busiest
                .replace("{agent}", agent)
                .replace("{n}", &s.calls.to_string()),
        );
    }

    for (agent, s) in rows {
        if s.sessions == 1 {
            out.push(
                lp.insight_one_shot
                    .replace("{agent}", agent)
                    .replace("{n}", &s.calls.to_string()),
            );
        }
    }

    for (agent, s) in rows {
        if s.calls == 0 {
            continue;
        }
        let pct = (s.writes * 100) / s.calls;
        if pct as u32 > WRITE_HEAVY_PCT {
            out.push(
                lp.insight_write_heavy
                    .replace("{agent}", agent)
                    .replace("{pct}", &pct.to_string()),
            );
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use fluxmirror_store::SqliteStore;
    use rusqlite::params;
    use tempfile::TempDir;

    fn fixture_db(rows: &[(&str, &str, &str, &str)]) -> (TempDir, PathBuf) {
        // (ts, agent, tool, session)
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("events.db");
        let _store = SqliteStore::open(&path).unwrap();
        let conn = rusqlite::Connection::open(&path).unwrap();
        for (ts, agent, tool, session) in rows {
            conn.execute(
                "INSERT INTO agent_events \
                 (ts, agent, session, tool, tool_canonical, tool_class, detail, \
                  cwd, host, user, schema_version, raw_json) \
                 VALUES (?1, ?2, ?3, ?4, ?4, 'Other', 'd', '/tmp', 'h', 'u', 1, '{}')",
                params![ts, agent, session, tool],
            )
            .unwrap();
        }
        (dir, path)
    }

    #[test]
    fn collect_stats_aggregates_calls_sessions_dominant_tool() {
        let (_d, path) = fixture_db(&[
            ("2026-04-26T10:00:00Z", "claude-code", "Bash", "s1"),
            ("2026-04-26T11:00:00Z", "claude-code", "Bash", "s1"),
            ("2026-04-26T12:00:00Z", "claude-code", "Edit", "s2"),
            ("2026-04-26T13:00:00Z", "gemini-cli", "read_file", "g1"),
        ]);
        let conn = open_db_readonly(&path).unwrap();
        let tz: Tz = "UTC".parse().unwrap();
        let start = "2026-04-26T00:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let end = "2026-04-27T00:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let stats = collect_stats(&conn, tz, start, end).unwrap();

        let claude = stats.get("claude-code").unwrap();
        assert_eq!(claude.calls, 3);
        assert_eq!(claude.sessions, 2);
        assert_eq!(claude.dominant_tool, "Bash");
        assert_eq!(claude.shells, 2, "two Bash rows");
        assert_eq!(claude.writes, 1, "one Edit row");
        assert_eq!(claude.active_days, 1);

        let gemini = stats.get("gemini-cli").unwrap();
        assert_eq!(gemini.calls, 1);
        assert_eq!(gemini.sessions, 1);
        assert_eq!(gemini.reads, 1);
    }

    #[test]
    fn render_human_handles_empty_window() {
        let lp = pack("english");
        let stats = BTreeMap::new();
        let s = render_human(
            lp,
            "UTC",
            NaiveDate::from_ymd_opt(2026, 4, 21).unwrap(),
            NaiveDate::from_ymd_opt(2026, 4, 27).unwrap(),
            &stats,
        );
        assert!(s.contains("No agent activity in the last 7 days."));
    }

    #[test]
    fn render_human_includes_table_and_busiest_insight() {
        let lp = pack("english");
        let mut stats = BTreeMap::new();
        stats.insert(
            "claude-code".into(),
            AgentStat {
                calls: 100,
                sessions: 5,
                active_days: 3,
                dominant_tool: "Bash".into(),
                writes: 20,
                reads: 30,
                shells: 50,
            },
        );
        stats.insert(
            "gemini-cli".into(),
            AgentStat {
                calls: 10,
                sessions: 1,
                active_days: 1,
                dominant_tool: "read_file".into(),
                writes: 1,
                reads: 8,
                shells: 1,
            },
        );
        let s = render_human(
            lp,
            "Asia/Seoul",
            NaiveDate::from_ymd_opt(2026, 4, 21).unwrap(),
            NaiveDate::from_ymd_opt(2026, 4, 27).unwrap(),
            &stats,
        );
        assert!(s.contains("Agent Roster"));
        assert!(s.contains("Asia/Seoul"));
        assert!(s.contains("claude-code"));
        assert!(s.contains("gemini-cli"));
        assert!(s.contains("| 100 |"));
        assert!(
            s.contains("claude-code is the busiest"),
            "missing busiest insight in:\n{s}"
        );
        assert!(
            s.contains("gemini-cli ran a single session"),
            "missing one-shot insight in:\n{s}"
        );
    }

    #[test]
    fn write_heavy_rule_fires_above_threshold() {
        let lp = pack("english");
        let mut stats = BTreeMap::new();
        stats.insert(
            "qwen-code".into(),
            AgentStat {
                calls: 10,
                sessions: 2,
                active_days: 2,
                dominant_tool: "Edit".into(),
                writes: 8,
                reads: 1,
                shells: 1,
            },
        );
        let s = render_human(
            lp,
            "UTC",
            NaiveDate::from_ymd_opt(2026, 4, 21).unwrap(),
            NaiveDate::from_ymd_opt(2026, 4, 27).unwrap(),
            &stats,
        );
        assert!(
            s.contains("qwen-code is write-heavy"),
            "expected write-heavy insight in:\n{s}"
        );
    }

    #[test]
    fn korean_pack_uses_translated_strings() {
        let lp = pack("korean");
        let mut stats = BTreeMap::new();
        stats.insert(
            "claude-code".into(),
            AgentStat {
                calls: 5,
                sessions: 1,
                active_days: 1,
                dominant_tool: "Bash".into(),
                writes: 1,
                reads: 2,
                shells: 2,
            },
        );
        let s = render_human(
            lp,
            "Asia/Seoul",
            NaiveDate::from_ymd_opt(2026, 4, 21).unwrap(),
            NaiveDate::from_ymd_opt(2026, 4, 27).unwrap(),
            &stats,
        );
        assert!(s.contains("에이전트 명세"), "expected ko title in:\n{s}");
        assert!(s.contains("호출"));
        assert!(s.contains("인사이트"));
    }
}
