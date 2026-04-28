// Single-pass SQL aggregators that produce the report DTOs.
//
// Phase 3 M2: the SQL queries that used to live inside
// `fluxmirror-cli/src/cmd/report/{day,week}.rs` are pulled here so the
// CLI text reports and the studio JSON API both consume the same
// extraction code. The CLI keeps its renderer-friendly map-based types
// as a thin adapter on top of these DTOs.
//
// Sort order across every list is canonical: count desc, then key asc
// alphabetical as the deterministic tiebreak. Consumers can take a
// prefix without re-sorting.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use chrono::{DateTime, Datelike, Duration, NaiveDate, Timelike, Utc};
use chrono_tz::Tz;
use rusqlite::Connection;

use super::dto::{
    AgentCount, AgentTouchCount, ContextEvent, DayRow, FileTouch, HourBucket, MethodCount,
    NowSnapshot, PathCount, ProvenanceData, ProvenanceEvent, ShellEvent, ToolMixEntry, TodayData,
    WeekData, WindowRange,
};

/// Tool names that count as "writes" — the file-mutating action class.
pub const WRITE_TOOLS: &[&str] = &[
    "Edit",
    "Write",
    "MultiEdit",
    "edit_file",
    "write_file",
    "replace",
];

/// Tool names that count as "reads" — file inspection without mutation.
pub const READ_TOOLS: &[&str] = &["Read", "read_file", "read_many_files"];

/// Tool names that invoke a shell command.
pub const SHELL_TOOLS: &[&str] = &["Bash", "run_shell_command"];

/// Returns `true` if `tool` is in [`WRITE_TOOLS`].
pub fn is_write(tool: &str) -> bool {
    WRITE_TOOLS.contains(&tool)
}

/// Returns `true` if `tool` is in [`READ_TOOLS`].
pub fn is_read(tool: &str) -> bool {
    READ_TOOLS.contains(&tool)
}

/// Returns `true` if `tool` is in [`SHELL_TOOLS`].
pub fn is_shell(tool: &str) -> bool {
    SHELL_TOOLS.contains(&tool)
}

/// Maximum unicode characters retained in a shell-command detail snippet
/// before truncation. Keeps the "Shell commands" table from blowing up
/// on giant heredocs.
const SHELL_DETAIL_MAX_CHARS: usize = 80;

/// Collect a one-day snapshot. `agent_filter`, when `Some`, restricts
/// every aggregation to rows where `agent = ?name`; the MCP-traffic
/// section is skipped in that case (the `events` table has no agent
/// column to filter on).
pub fn collect_today(
    conn: &Connection,
    tz: &Tz,
    range: WindowRange,
    agent_filter: Option<&str>,
) -> Result<TodayData, String> {
    let raw = aggregate_day_raw(conn, tz, &range, agent_filter)?;

    let mut data = TodayData {
        date: range.anchor_date,
        tz: range.tz.clone(),
        total_events: raw.total_events,
        writes_total: raw.writes_total,
        reads_total: raw.reads_total,
        ..Default::default()
    };

    data.agents = build_agents(&raw.agents);
    data.files_edited = build_files_edited(&raw.files_edited);
    data.files_read = build_path_counts(&raw.files_read);
    data.shells = build_shells(&raw.shells);
    data.cwds = build_path_counts(&raw.cwds);
    data.mcp_methods = build_method_counts(&raw.mcp_methods);
    data.tool_mix = build_tool_mix(&raw.tool_mix);
    data.hours = build_hour_buckets(&raw.hours);
    data.distinct_files = raw.distinct_files.iter().cloned().collect();

    Ok(data)
}

/// Collect a 7-day rolling snapshot. `agent_filter` works the same way
/// as in [`collect_today`]; when set, the MCP traffic count is forced
/// to 0 because the `events` table is unfiltered by agent.
pub fn collect_week(
    conn: &Connection,
    tz: &Tz,
    range: WindowRange,
    agent_filter: Option<&str>,
) -> Result<WeekData, String> {
    let raw = aggregate_week_raw(conn, tz, &range, agent_filter)?;

    let mut data = WeekData {
        range_start: range.anchor_date,
        range_end: range.anchor_date + Duration::days(6),
        tz: range.tz.clone(),
        total_events: raw.total_events,
        writes_total: raw.writes_total,
        reads_total: raw.reads_total,
        ..Default::default()
    };

    data.agents = build_agents(&raw.agents);
    data.files_edited = build_files_edited(&raw.files_edited);
    data.files_read = build_path_counts(&raw.files_read);
    data.cwds = build_path_counts(&raw.cwds);
    data.tool_mix = build_tool_mix(&raw.tool_mix);
    data.daily = raw
        .days_in_window
        .iter()
        .map(|d| DayRow {
            date: *d,
            calls: raw.daily_calls.get(d).copied().unwrap_or(0),
        })
        .collect();
    data.heatmap = raw.heatmap.iter().map(|row| row.to_vec()).collect();
    data.shell_counts = build_path_counts(&raw.shell_counts);
    data.mcp_count = if agent_filter.is_some() {
        0
    } else {
        count_mcp_events(conn, range.start_utc, range.end_utc).unwrap_or(0)
    };

    Ok(data)
}

/// Latest-event snapshot. Returns `Ok(None)` when `agent_events` is
/// empty.
pub fn collect_now(conn: &Connection) -> Result<Option<NowSnapshot>, String> {
    let row: Option<(String, String, String, String, String)> = match conn.prepare(
        "SELECT ts, agent, COALESCE(tool, '') AS tool, \
                COALESCE(detail, '') AS detail, COALESCE(cwd, '') AS cwd \
         FROM agent_events ORDER BY ts DESC LIMIT 1",
    ) {
        Ok(mut stmt) => stmt
            .query_row([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                ))
            })
            .ok(),
        Err(e) => return Err(format!("prepare(now): {e}")),
    };

    let Some((ts, agent, tool, detail, cwd)) = row else {
        return Ok(None);
    };
    let latest_ts_utc = DateTime::parse_from_rfc3339(&ts)
        .map_err(|e| format!("parse(latest ts): {e}"))?
        .with_timezone(&Utc);

    // Trailing 60 minutes from the latest event (not from "now") so the
    // snapshot is meaningful even when the user hasn't run a tool in a
    // while: the dashboard shows the burst of activity around the most
    // recent event.
    let hour_start = latest_ts_utc - Duration::minutes(60);
    let start_str = hour_start.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let end_str = (latest_ts_utc + Duration::seconds(1))
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

    let mut hour_agents: BTreeMap<String, RawAgent> = BTreeMap::new();
    let mut hour_total: u64 = 0;
    if let Ok(mut stmt) = conn.prepare(
        "SELECT agent, COALESCE(session, '') AS session, COALESCE(tool, '') AS tool \
         FROM agent_events WHERE ts >= ?1 AND ts < ?2",
    ) {
        if let Ok(rows) = stmt.query_map([&start_str, &end_str], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        }) {
            for row in rows.flatten() {
                let (a, s, t) = row;
                hour_total += 1;
                let entry = hour_agents.entry(a).or_default();
                entry.calls += 1;
                if !s.is_empty() {
                    entry.sessions.insert(s);
                }
                if !t.is_empty() {
                    *entry.tool_counts.entry(t).or_default() += 1;
                }
            }
        }
    }

    Ok(Some(NowSnapshot {
        latest_ts_utc,
        latest_agent: agent,
        latest_tool: tool,
        latest_detail: detail,
        latest_cwd: cwd,
        last_hour_total: hour_total,
        last_hour_agents: build_agents(&hour_agents),
    }))
}

/// Width of the before/after context window around a file touch.
const PROVENANCE_WINDOW_MINUTES: i64 = 5;

/// Maximum number of context events kept on each side of a touch.
const PROVENANCE_CONTEXT_LIMIT: usize = 5;

/// Collect every `agent_events` row whose `detail` equals `path`, plus
/// the ±5 minute context window around each touch. Returns an empty
/// `ProvenanceData` (with `total_touches = 0`) when the path was never
/// touched — the caller decides whether that maps to 200 or 404.
pub fn collect_provenance(conn: &Connection, path: &str) -> Result<ProvenanceData, String> {
    let mut data = ProvenanceData {
        path: path.to_string(),
        ..Default::default()
    };

    let touches: Vec<(i64, String, String, String, String, Option<String>)> = {
        let mut stmt = conn
            .prepare(
                "SELECT id, ts, agent, \
                        COALESCE(tool_canonical, COALESCE(tool, '')) AS tool, \
                        COALESCE(tool_class, '') AS tool_class, detail \
                 FROM agent_events WHERE detail = ?1 ORDER BY ts ASC, id ASC",
            )
            .map_err(|e| format!("prepare(provenance touches): {e}"))?;
        let mapped = stmt
            .query_map([path], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, Option<String>>(5)?,
                ))
            })
            .map_err(|e| format!("query(provenance touches): {e}"))?;
        let mut out = Vec::new();
        for row in mapped {
            out.push(row.map_err(|e| format!("row(provenance touches): {e}"))?);
        }
        out
    };

    if touches.is_empty() {
        return Ok(data);
    }

    data.total_touches = touches.len() as i64;

    let mut per_agent: BTreeMap<String, i64> = BTreeMap::new();
    for (_, _, agent, _, _, _) in &touches {
        *per_agent.entry(agent.clone()).or_default() += 1;
    }
    let mut agents: Vec<AgentTouchCount> = per_agent
        .into_iter()
        .map(|(agent, count)| AgentTouchCount { agent, count })
        .collect();
    agents.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.agent.cmp(&b.agent)));
    data.agents = agents;

    let mut events: Vec<ProvenanceEvent> = Vec::with_capacity(touches.len());
    for (id, ts, agent, tool, tool_class, detail) in touches {
        let touch_dt = match DateTime::parse_from_rfc3339(&ts) {
            Ok(dt) => dt.with_timezone(&Utc),
            Err(_) => {
                // Unparseable ts — emit the touch with empty context
                // rather than failing the whole request.
                events.push(ProvenanceEvent {
                    ts,
                    agent,
                    tool,
                    tool_class,
                    detail,
                    before_context: Vec::new(),
                    after_context: Vec::new(),
                });
                continue;
            }
        };
        let window_start = touch_dt - Duration::minutes(PROVENANCE_WINDOW_MINUTES);
        let window_end = touch_dt + Duration::minutes(PROVENANCE_WINDOW_MINUTES);
        let start_str = window_start.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let end_str = window_end.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

        let mut before_context: Vec<ContextEvent> = Vec::new();
        let mut after_context: Vec<ContextEvent> = Vec::new();

        let mut stmt = conn
            .prepare(
                "SELECT ts, agent, \
                        COALESCE(tool_canonical, COALESCE(tool, '')) AS tool, detail \
                 FROM agent_events \
                 WHERE id <> ?1 AND ts >= ?2 AND ts <= ?3 \
                 ORDER BY ts ASC, id ASC",
            )
            .map_err(|e| format!("prepare(provenance context): {e}"))?;
        let mapped = stmt
            .query_map(
                rusqlite::params![id, &start_str, &end_str],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, Option<String>>(3)?,
                    ))
                },
            )
            .map_err(|e| format!("query(provenance context): {e}"))?;
        for row in mapped {
            let (ctx_ts, ctx_agent, ctx_tool, ctx_detail) =
                row.map_err(|e| format!("row(provenance context): {e}"))?;
            let ctx_dt = match DateTime::parse_from_rfc3339(&ctx_ts) {
                Ok(dt) => dt.with_timezone(&Utc),
                Err(_) => continue,
            };
            let event = ContextEvent {
                ts: ctx_ts,
                agent: ctx_agent,
                tool: ctx_tool,
                detail: ctx_detail,
            };
            if ctx_dt < touch_dt {
                before_context.push(event);
            } else if ctx_dt > touch_dt {
                after_context.push(event);
            }
        }

        // Keep the events closest to the touch — for `before_context`
        // that's the tail (most recent), for `after_context` the head
        // (earliest after the touch).
        if before_context.len() > PROVENANCE_CONTEXT_LIMIT {
            let drop = before_context.len() - PROVENANCE_CONTEXT_LIMIT;
            before_context.drain(0..drop);
        }
        if after_context.len() > PROVENANCE_CONTEXT_LIMIT {
            after_context.truncate(PROVENANCE_CONTEXT_LIMIT);
        }

        events.push(ProvenanceEvent {
            ts,
            agent,
            tool,
            tool_class,
            detail,
            before_context,
            after_context,
        });
    }
    data.events = events;

    Ok(data)
}

/// Best-effort MCP traffic counter — falls back to 0 when the `events`
/// table doesn't exist (legacy DB without the proxy migration).
fn count_mcp_events(
    conn: &Connection,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Result<u64, String> {
    let start_ms = start.timestamp_millis();
    let end_ms = end.timestamp_millis();
    let mut stmt = match conn
        .prepare("SELECT COUNT(*) FROM events WHERE ts_ms >= ?1 AND ts_ms < ?2")
    {
        Ok(s) => s,
        Err(_) => return Ok(0),
    };
    let n: i64 = stmt
        .query_row([start_ms, end_ms], |r| r.get(0))
        .unwrap_or(0);
    Ok(n.max(0) as u64)
}

/// Per-agent intermediate state. Distinct from [`AgentCount`] — keeps
/// hash-keyed maps so we can count without `find()` per row.
#[derive(Debug, Default, Clone)]
pub(crate) struct RawAgent {
    pub(crate) calls: u64,
    pub(crate) sessions: BTreeSet<String>,
    pub(crate) active_dates: BTreeSet<NaiveDate>,
    pub(crate) tool_counts: HashMap<String, u64>,
}

/// Pre-DTO scratch space for the day collector. Mirrors the CLI's
/// historical `DayStats` field layout so the CLI's wrapper can adapt
/// it back into its existing renderer-friendly types without losing
/// information.
#[derive(Debug, Default)]
pub(crate) struct RawDay {
    pub total_events: u64,
    pub agents: BTreeMap<String, RawAgent>,
    pub files_edited: HashMap<(String, String), u64>,
    pub files_read: HashMap<String, u64>,
    pub shells: Vec<ShellEvent>,
    pub cwds: HashMap<String, u64>,
    pub mcp_methods: HashMap<String, u64>,
    pub tool_mix: HashMap<String, u64>,
    pub hours: [u64; 24],
    pub reads_total: u64,
    pub writes_total: u64,
    pub distinct_files: BTreeSet<String>,
}

/// Pre-DTO scratch space for the week collector.
#[derive(Debug, Default)]
pub(crate) struct RawWeek {
    pub total_events: u64,
    pub agents: BTreeMap<String, RawAgent>,
    pub files_edited: HashMap<(String, String), u64>,
    pub files_read: HashMap<String, u64>,
    pub cwds: HashMap<String, u64>,
    pub tool_mix: HashMap<String, u64>,
    pub days_in_window: Vec<NaiveDate>,
    pub daily_calls: HashMap<NaiveDate, u64>,
    pub reads_total: u64,
    pub writes_total: u64,
    pub heatmap: [[u32; 24]; 7],
    pub shell_counts: HashMap<String, u64>,
}

/// Single SQL pass over `agent_events` plus optional MCP-method query.
fn aggregate_day_raw(
    conn: &Connection,
    tz: &Tz,
    range: &WindowRange,
    agent_filter: Option<&str>,
) -> Result<RawDay, String> {
    let start_str = range
        .start_utc
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let end_str = range
        .end_utc
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let start_ms = range.start_utc.timestamp_millis();
    let end_ms = range.end_utc.timestamp_millis();

    let mut day = RawDay::default();

    let row_t = |r: &rusqlite::Row<'_>| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, String>(5)?,
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

        let entry = day.agents.entry(agent).or_default();
        entry.calls += 1;
        if !session.is_empty() {
            entry.sessions.insert(session);
        }
        if !tool.is_empty() {
            *entry.tool_counts.entry(tool.clone()).or_default() += 1;
        }

        if !tool.is_empty() {
            *day.tool_mix.entry(tool.clone()).or_default() += 1;
        }
        if !cwd.is_empty() {
            *day.cwds.entry(cwd).or_default() += 1;
        }

        let tool_str = tool.as_str();
        if is_write(tool_str) && !detail.is_empty() {
            *day.files_edited
                .entry((detail.clone(), tool.clone()))
                .or_default() += 1;
            day.writes_total += 1;
            day.distinct_files.insert(detail.clone());
        } else if is_read(tool_str) && !detail.is_empty() {
            *day.files_read.entry(detail.clone()).or_default() += 1;
            day.reads_total += 1;
            day.distinct_files.insert(detail.clone());
        } else if is_shell(tool_str) {
            if let Ok(dt) = DateTime::parse_from_rfc3339(&ts) {
                let dt_utc = dt.with_timezone(&Utc);
                let local = dt_utc.with_timezone(tz);
                let time_local = format!("{:02}:{:02}", local.hour(), local.minute());
                day.shells.push(ShellEvent {
                    time_local,
                    detail: truncate_chars(&detail, SHELL_DETAIL_MAX_CHARS),
                    ts_utc: dt_utc,
                });
            }
        } else if is_write(tool_str) {
            day.writes_total += 1;
        } else if is_read(tool_str) {
            day.reads_total += 1;
        }

        if let Ok(dt) = DateTime::parse_from_rfc3339(&ts) {
            let local = dt.with_timezone(tz);
            let h = local.hour() as usize;
            day.hours[h] = day.hours[h].saturating_add(1);
            entry.active_dates.insert(local.date_naive());
        }
    }

    day.shells.sort_by_key(|s| s.ts_utc);

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

fn aggregate_week_raw(
    conn: &Connection,
    tz: &Tz,
    range: &WindowRange,
    agent_filter: Option<&str>,
) -> Result<RawWeek, String> {
    let start_str = range
        .start_utc
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let end_str = range
        .end_utc
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

    let mut stats = RawWeek::default();

    for i in 0..7 {
        let d = range.anchor_date + Duration::days(i);
        stats.days_in_window.push(d);
        stats.daily_calls.insert(d, 0);
    }

    let row_t = |r: &rusqlite::Row<'_>| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, String>(5)?,
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
            for r in mapped {
                out.push(r.map_err(|e| format!("row(events): {e}"))?);
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
            for r in mapped {
                out.push(r.map_err(|e| format!("row(events): {e}"))?);
            }
            out
        }
    };

    for (ts, agent, session, tool, detail, cwd) in collected {
        stats.total_events += 1;

        let entry = stats.agents.entry(agent).or_default();
        entry.calls += 1;
        if !session.is_empty() {
            entry.sessions.insert(session);
        }
        if !tool.is_empty() {
            *entry.tool_counts.entry(tool.clone()).or_default() += 1;
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
            if !detail.is_empty() {
                let snippet: String = detail.chars().take(SHELL_DETAIL_MAX_CHARS).collect();
                *stats.shell_counts.entry(snippet).or_default() += 1;
            }
        } else if is_write(tool_str) {
            stats.writes_total += 1;
        } else if is_read(tool_str) {
            stats.reads_total += 1;
        }

        if let Ok(dt) = DateTime::parse_from_rfc3339(&ts) {
            let local = dt.with_timezone(tz);
            let local_date = local.date_naive();
            if let Some(c) = stats.daily_calls.get_mut(&local_date) {
                *c += 1;
            }
            entry.active_dates.insert(local_date);
            let dow = local.weekday().num_days_from_monday() as usize;
            let hour = local.hour() as usize;
            if dow < 7 && hour < 24 {
                stats.heatmap[dow][hour] = stats.heatmap[dow][hour].saturating_add(1);
            }
        }
    }

    Ok(stats)
}

fn build_agents(raw: &BTreeMap<String, RawAgent>) -> Vec<AgentCount> {
    let mut out: Vec<AgentCount> = raw
        .iter()
        .map(|(name, row)| {
            let mut sessions: Vec<String> = row.sessions.iter().cloned().collect();
            sessions.sort();
            let mut active_days: Vec<NaiveDate> = row.active_dates.iter().cloned().collect();
            active_days.sort();
            let top_tool = row
                .tool_counts
                .iter()
                .max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0)))
                .map(|(t, _)| t.clone())
                .unwrap_or_default();
            AgentCount {
                agent: name.clone(),
                calls: row.calls,
                sessions,
                active_days,
                top_tool,
            }
        })
        .collect();
    out.sort_by(|a, b| b.calls.cmp(&a.calls).then_with(|| a.agent.cmp(&b.agent)));
    out
}

fn build_files_edited(raw: &HashMap<(String, String), u64>) -> Vec<FileTouch> {
    let mut out: Vec<FileTouch> = raw
        .iter()
        .map(|((path, tool), n)| FileTouch {
            path: path.clone(),
            tool: tool.clone(),
            count: *n,
        })
        .collect();
    out.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| a.path.cmp(&b.path))
            .then_with(|| a.tool.cmp(&b.tool))
    });
    out
}

fn build_path_counts(raw: &HashMap<String, u64>) -> Vec<PathCount> {
    let mut out: Vec<PathCount> = raw
        .iter()
        .map(|(path, n)| PathCount {
            path: path.clone(),
            count: *n,
        })
        .collect();
    out.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.path.cmp(&b.path)));
    out
}

fn build_method_counts(raw: &HashMap<String, u64>) -> Vec<MethodCount> {
    let mut out: Vec<MethodCount> = raw
        .iter()
        .map(|(method, n)| MethodCount {
            method: method.clone(),
            count: *n,
        })
        .collect();
    out.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.method.cmp(&b.method)));
    out
}

fn build_tool_mix(raw: &HashMap<String, u64>) -> Vec<ToolMixEntry> {
    let mut out: Vec<ToolMixEntry> = raw
        .iter()
        .map(|(tool, n)| ToolMixEntry {
            tool: tool.clone(),
            count: *n,
        })
        .collect();
    out.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.tool.cmp(&b.tool)));
    out
}

fn build_hour_buckets(hours: &[u64; 24]) -> Vec<HourBucket> {
    (0..24u8)
        .map(|h| HourBucket {
            hour: h,
            count: hours[h as usize],
        })
        .collect()
}

fn build_shells(shells: &[ShellEvent]) -> Vec<ShellEvent> {
    let mut out = shells.to_vec();
    out.sort_by_key(|s| s.ts_utc);
    out
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    s.chars().take(max).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::{params, Connection, OpenFlags};
    use tempfile::TempDir;

    fn schema(conn: &Connection) {
        conn.execute_batch(
            "CREATE TABLE schema_meta (version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL);
             CREATE TABLE agent_events (
               id INTEGER PRIMARY KEY AUTOINCREMENT,
               ts TEXT NOT NULL,
               agent TEXT NOT NULL,
               session TEXT,
               tool TEXT,
               tool_canonical TEXT,
               tool_class TEXT,
               detail TEXT,
               cwd TEXT,
               host TEXT,
               user TEXT,
               schema_version INTEGER NOT NULL DEFAULT 1,
               raw_json TEXT
             );
             CREATE TABLE events (
               id INTEGER PRIMARY KEY AUTOINCREMENT,
               ts_ms INTEGER NOT NULL,
               direction TEXT NOT NULL CHECK (direction IN ('c2s','s2c')),
               method TEXT,
               message_json TEXT NOT NULL,
               server_name TEXT NOT NULL
             );",
        )
        .unwrap();
    }

    fn fixture(rows: &[(&str, &str, &str, &str, &str, &str)]) -> (TempDir, Connection) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("events.db");
        let conn = Connection::open(&path).unwrap();
        schema(&conn);
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
        let ro = Connection::open_with_flags(
            &path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .unwrap();
        (dir, ro)
    }

    #[test]
    fn collect_today_aggregates_basic_counts() {
        let (_d, conn) = fixture(&[
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
        let tz: Tz = "UTC".parse().unwrap();
        let range = WindowRange {
            start_utc: "2026-04-26T00:00:00Z".parse::<DateTime<Utc>>().unwrap(),
            end_utc: "2026-04-27T00:00:00Z".parse::<DateTime<Utc>>().unwrap(),
            anchor_date: NaiveDate::from_ymd_opt(2026, 4, 26).unwrap(),
            tz: "UTC".to_string(),
        };
        let data = collect_today(&conn, &tz, range, None).unwrap();
        assert_eq!(data.total_events, 3);
        assert_eq!(data.agents.len(), 2);
        assert_eq!(data.shells.len(), 1);
        assert_eq!(data.writes_total, 1);
        assert_eq!(data.reads_total, 1);
        assert_eq!(data.cwds.len(), 2);
        assert!(data.distinct_files.contains(&"src/foo.rs".to_string()));
        assert!(data.distinct_files.contains(&"src/bar.rs".to_string()));
        assert_eq!(data.hours.len(), 24);
        assert_eq!(data.hours[1].count, 1);
        assert_eq!(data.hours[2].count, 1);
        assert_eq!(data.hours[3].count, 1);
    }

    #[test]
    fn collect_today_filter_scopes_to_one_agent() {
        let (_d, conn) = fixture(&[
            (
                "2026-04-26T01:00:00Z",
                "claude-code",
                "Edit",
                "s1",
                "a",
                "/p",
            ),
            (
                "2026-04-26T02:00:00Z",
                "gemini-cli",
                "edit_file",
                "g1",
                "b",
                "/q",
            ),
        ]);
        let tz: Tz = "UTC".parse().unwrap();
        let range = WindowRange {
            start_utc: "2026-04-26T00:00:00Z".parse::<DateTime<Utc>>().unwrap(),
            end_utc: "2026-04-27T00:00:00Z".parse::<DateTime<Utc>>().unwrap(),
            anchor_date: NaiveDate::from_ymd_opt(2026, 4, 26).unwrap(),
            tz: "UTC".to_string(),
        };
        let data = collect_today(&conn, &tz, range, Some("claude-code")).unwrap();
        assert_eq!(data.total_events, 1);
        assert_eq!(data.agents.len(), 1);
        assert_eq!(data.agents[0].agent, "claude-code");
    }

    #[test]
    fn collect_week_includes_zero_days() {
        let (_d, conn) = fixture(&[
            (
                "2026-04-21T01:00:00Z",
                "claude-code",
                "Edit",
                "s1",
                "a",
                "/p",
            ),
            (
                "2026-04-22T02:00:00Z",
                "claude-code",
                "Edit",
                "s1",
                "a",
                "/p",
            ),
        ]);
        let tz: Tz = "UTC".parse().unwrap();
        let range = WindowRange {
            start_utc: "2026-04-21T00:00:00Z".parse::<DateTime<Utc>>().unwrap(),
            end_utc: "2026-04-28T00:00:00Z".parse::<DateTime<Utc>>().unwrap(),
            anchor_date: NaiveDate::from_ymd_opt(2026, 4, 21).unwrap(),
            tz: "UTC".to_string(),
        };
        let data = collect_week(&conn, &tz, range, None).unwrap();
        assert_eq!(data.daily.len(), 7);
        assert_eq!(data.daily[0].calls, 1);
        assert_eq!(data.daily[1].calls, 1);
        assert_eq!(data.daily[6].calls, 0);
        assert_eq!(data.heatmap.len(), 7);
        assert_eq!(data.heatmap[0].len(), 24);
    }

    #[test]
    fn collect_now_returns_none_for_empty_db() {
        let (_d, conn) = fixture(&[]);
        let snap = collect_now(&conn).unwrap();
        assert!(snap.is_none());
    }

    #[test]
    fn collect_now_returns_latest_event() {
        let (_d, conn) = fixture(&[
            (
                "2026-04-26T01:00:00Z",
                "claude-code",
                "Edit",
                "s1",
                "src/a.rs",
                "/p",
            ),
            (
                "2026-04-26T02:30:00Z",
                "gemini-cli",
                "edit_file",
                "g1",
                "README.md",
                "/q",
            ),
            (
                "2026-04-26T02:35:00Z",
                "claude-code",
                "Bash",
                "s1",
                "ls",
                "/p",
            ),
        ]);
        let snap = collect_now(&conn).unwrap().unwrap();
        assert_eq!(snap.latest_agent, "claude-code");
        assert_eq!(snap.latest_tool, "Bash");
        assert_eq!(snap.latest_detail, "ls");
        // last hour from 02:35 → covers 01:35..02:35 + 1s. The 02:30
        // gemini event and the 02:35 claude event both qualify; the
        // 01:00 event does not.
        assert_eq!(snap.last_hour_total, 2);
        assert_eq!(snap.last_hour_agents.len(), 2);
    }
}
