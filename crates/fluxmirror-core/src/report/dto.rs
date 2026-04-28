// Public report DTOs shared by every consumer of the fluxmirror SQLite
// store: the CLI's text/HTML reports and the studio's JSON API.
//
// Phase 3 M2: data.rs collects rows in a single SQL pass and folds them
// into one of these structs. Both consumers then format / serialise the
// same struct — no SQL is duplicated downstream.
//
// All structs derive serde::Serialize so the studio's `axum::Json<...>`
// handlers can return them as-is. Consumers that want a typed
// roundtrip can also derive Deserialize from the JSON shape.

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

/// UTC bounds for a report window plus the local-date label that
/// belongs in the rendered title. Kept as plain fields so callers can
/// build it from any window source (`today_range`, `day_range`,
/// `week_range`, or a hand-picked custom window).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowRange {
    /// Inclusive UTC start of the window.
    pub start_utc: DateTime<Utc>,
    /// Exclusive UTC end of the window.
    pub end_utc: DateTime<Utc>,
    /// Local-timezone date that anchors the report. For day-shaped
    /// reports this is the report date. For week-shaped reports this
    /// is the first day of the 7-day window.
    pub anchor_date: NaiveDate,
    /// IANA tz label (e.g. `Asia/Seoul`, `UTC`). Echoed into the
    /// rendered output unchanged.
    pub tz: String,
}

/// One row of the per-agent activity table. `sessions` is the distinct
/// list of session ids the agent emitted in the window; `active_days`
/// is the distinct list of local dates the agent had at least one
/// event on (always 1 for day-shaped windows, up to 7 for week-shaped).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCount {
    pub agent: String,
    pub calls: u64,
    pub sessions: Vec<String>,
    pub active_days: Vec<NaiveDate>,
    /// Tool name with the highest count for this agent (alphabetical
    /// tiebreak). Empty when the agent emitted no tool name.
    pub top_tool: String,
}

/// One (path, tool) bucket from the files-edited table. Path is the
/// user-facing file path (the `detail` column from `agent_events`),
/// tool is the raw tool name (e.g. `Edit`, `edit_file`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileTouch {
    pub path: String,
    pub tool: String,
    pub count: u64,
}

/// Generic (path, count) bucket. Used by files-read, cwds, and the
/// week's shell-command snippet table (where `path` is a 80-char
/// truncation of the shell command).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathCount {
    pub path: String,
    pub count: u64,
}

/// One bar of the tool-mix table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolMixEntry {
    pub tool: String,
    pub count: u64,
}

/// One row of the MCP traffic table (sourced from the proxy's `events`
/// table, not `agent_events`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MethodCount {
    pub method: String,
    pub count: u64,
}

/// One shell-command event in the day's chronological shell list.
/// `time_local` is `HH:MM` in the user's tz; `detail` is the truncated
/// command string (max 80 unicode chars); `ts_utc` is the original UTC
/// timestamp so the studio can sort or render relative times.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellEvent {
    pub time_local: String,
    pub detail: String,
    pub ts_utc: DateTime<Utc>,
}

/// One bucket of the 24-bar hour-of-day chart. Empty hours are still
/// emitted (count = 0) so the renderer can draw a fixed-width chart.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct HourBucket {
    pub hour: u8,
    pub count: u64,
}

/// One row of the per-day totals chart used by the week report.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct DayRow {
    pub date: NaiveDate,
    pub calls: u64,
}

/// All aggregates for a single-day window. Mirrors the CLI's text-mode
/// report sections one-for-one. Lists are pre-sorted in a canonical
/// order (count desc, key asc as tiebreak) so consumers can take a
/// prefix without re-sorting.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TodayData {
    pub date: NaiveDate,
    pub tz: String,
    pub total_events: u64,
    pub agents: Vec<AgentCount>,
    pub files_edited: Vec<FileTouch>,
    pub files_read: Vec<PathCount>,
    pub shells: Vec<ShellEvent>,
    pub cwds: Vec<PathCount>,
    pub mcp_methods: Vec<MethodCount>,
    pub tool_mix: Vec<ToolMixEntry>,
    /// 24 entries, hour 0..=23. Always present (zero-filled).
    pub hours: Vec<HourBucket>,
    pub writes_total: u64,
    pub reads_total: u64,
    /// Distinct file paths touched by either a write or a read with
    /// non-empty detail. Sorted ascending.
    pub distinct_files: Vec<String>,
}

/// All aggregates for a 7-day rolling window.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WeekData {
    pub range_start: NaiveDate,
    pub range_end: NaiveDate,
    pub tz: String,
    pub total_events: u64,
    pub agents: Vec<AgentCount>,
    pub files_edited: Vec<FileTouch>,
    pub files_read: Vec<PathCount>,
    pub cwds: Vec<PathCount>,
    pub tool_mix: Vec<ToolMixEntry>,
    /// 7 rows, chronological. Days with no events still appear with
    /// `calls = 0`.
    pub daily: Vec<DayRow>,
    /// `[day_of_week][hour_of_day]` calls. Day index 0=Monday..6=Sunday
    /// (ISO 8601). Always exactly 7 outer rows of length 24.
    pub heatmap: Vec<Vec<u32>>,
    /// Truncated shell-command snippets bucketed by count.
    pub shell_counts: Vec<PathCount>,
    pub mcp_count: u64,
    pub writes_total: u64,
    pub reads_total: u64,
}

/// Latest-event snapshot rendered on the studio Home page's "Now"
/// panel. `None` is returned when the database has no `agent_events`
/// rows at all.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NowSnapshot {
    pub latest_ts_utc: DateTime<Utc>,
    pub latest_agent: String,
    pub latest_tool: String,
    pub latest_detail: String,
    pub latest_cwd: String,
    /// Total calls in the trailing 60 minutes (UTC).
    pub last_hour_total: u64,
    /// Per-agent breakdown of those trailing-60-minutes calls. Empty
    /// when the most recent event is older than 60 minutes.
    pub last_hour_agents: Vec<AgentCount>,
}
