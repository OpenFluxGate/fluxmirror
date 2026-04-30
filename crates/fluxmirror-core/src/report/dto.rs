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
    /// Cost overlay (Phase 3 M6). `None` when scoped by agent — the
    /// cost source for MCP traffic isn't filterable by agent so we
    /// suppress the figure rather than report a misleading total.
    #[serde(default)]
    pub cost: Option<CostSummary>,
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
    /// Cost overlay (Phase 3 M6). `None` when scoped by agent.
    #[serde(default)]
    pub cost: Option<CostSummary>,
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

/// Full provenance timeline for a single file path. Returned by the
/// studio's `/api/file?path=…` endpoint and rendered on the
/// `/file/<path>` page. When the path has no rows in `agent_events`
/// every list is empty and `total_touches` is zero — callers can use
/// that to render an empty-state without a separate 404 path.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProvenanceData {
    /// User-supplied file path. Echoed back so the frontend can render
    /// the title without keeping the original query param around.
    pub path: String,
    /// Total `agent_events` rows where `detail = path`.
    pub total_touches: i64,
    /// Per-agent touch counts, sorted by count desc with the agent
    /// name as deterministic tiebreak.
    pub agents: Vec<AgentTouchCount>,
    /// Each row from `agent_events` matching the path, in chronological
    /// order, decorated with the immediate before/after context window.
    pub events: Vec<ProvenanceEvent>,
}

/// Per-agent touch count for a single path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTouchCount {
    pub agent: String,
    pub count: i64,
}

/// A single `agent_events` row that touched the path, plus the events
/// the agent emitted in the ±5 minute window around the touch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvenanceEvent {
    /// ISO 8601 UTC timestamp of the touch.
    pub ts: String,
    pub agent: String,
    /// Canonicalised tool name (e.g. `Edit`, `Read`, `Bash`). Empty
    /// when the row's `tool_canonical` column is null.
    pub tool: String,
    /// Tool family (`Write`, `Read`, `Shell`, …). Empty when the row's
    /// `tool_class` column is null.
    pub tool_class: String,
    pub detail: Option<String>,
    /// Up to 5 events strictly before the touch within the 5-minute
    /// window, sorted by ts ascending.
    pub before_context: Vec<ContextEvent>,
    /// Up to 5 events strictly after the touch within the 5-minute
    /// window, sorted by ts ascending.
    pub after_context: Vec<ContextEvent>,
}

/// One row of provenance context (an event near a touch). Lighter
/// shape than [`ProvenanceEvent`] — context events do not recurse and
/// carry only the fields the timeline card renders.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextEvent {
    pub ts: String,
    pub agent: String,
    pub tool: String,
    pub detail: Option<String>,
}

/// Lifecycle classification for a clustered work session. Drives the
/// session list badge and the verb-prefix on the auto-generated name.
/// Order is stable and the variant strings are the JSON payload —
/// renames break the frontend and persisted snapshots.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SessionLifecycle {
    /// `git tag` + `git push` Bash detail in window.
    Shipping,
    /// Edit-heavy on a small file set (≤3 distinct files).
    Building,
    /// Edit-heavy across many files (cleanup pass).
    Polishing,
    /// 5+ identical `cargo test` / `pytest` / `npm test` / `go test` runs.
    Testing,
    /// Read-heavy (Read > Edit*2) with at least one shell call.
    Investigating,
    /// Mostly Bash chores, no real work signature.
    Idle,
}

/// One auto-named work session. Returned by `/api/sessions` (events
/// elided) and `/api/session/:id` (events populated).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    /// 8-character lowercase hex digest of `format!("{start}|{end}")`.
    /// Stable across runs given the same start/end timestamps.
    pub id: String,
    /// ISO 8601 UTC timestamp of the first event in the cluster.
    pub start: String,
    /// ISO 8601 UTC timestamp of the last event in the cluster.
    pub end: String,
    /// Distinct agent names that emitted at least one event in the
    /// session, sorted ascending.
    pub agents: Vec<String>,
    pub event_count: i64,
    /// Most-frequent `cwd` field in the cluster. `None` when every row
    /// had an empty cwd.
    pub dominant_cwd: Option<String>,
    /// Up to five most-touched file paths (Edit/Write tools), sorted by
    /// touch count desc with the path as deterministic tiebreak.
    pub top_files: Vec<String>,
    /// Tool counts for the cluster, sorted by count desc with the tool
    /// name as deterministic tiebreak.
    pub tool_mix: Vec<ToolMixEntry>,
    pub lifecycle: SessionLifecycle,
    /// Heuristic name of the form `<Verb>: <Object> (<Tail>)`. Same
    /// inputs always produce the same name.
    pub name: String,
    /// Per-event timeline. Empty in the list endpoint, populated in
    /// the detail endpoint. `serde` always emits the field so the
    /// TypeScript shape is invariant across endpoints.
    #[serde(default)]
    pub events: Vec<SessionEvent>,
}

/// One event inside a session's timeline. Lighter than
/// [`ProvenanceEvent`] — sessions do not carry per-event context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionEvent {
    /// ISO 8601 UTC timestamp.
    pub ts: String,
    pub agent: String,
    /// Raw tool name as stored in `agent_events.tool`. Empty when the
    /// row's tool column is null.
    pub tool: String,
    pub detail: Option<String>,
}

/// Full per-day timeline used by the studio's `/replay/<date>` page.
/// `events` is every `agent_events` row that fell inside the local day,
/// sorted by `ts` ascending. `minute_buckets` is a fixed-length
/// 1440-entry vector — one slot per minute of the local day,
/// zero-filled for empty minutes — so the heatmap renderer can index by
/// minute without bounds checks.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReplayDay {
    /// Local date the timeline anchors on, formatted `YYYY-MM-DD`.
    pub date: String,
    /// All events in the local day, chronological. `tool` and
    /// `tool_class` mirror the `agent_events` columns so the snapshot
    /// view can re-use the same row shape.
    pub events: Vec<ReplayEvent>,
    /// Exactly 1440 entries (`minute = 0..=1439`). Empty minutes are
    /// still present with `count = 0` so the renderer can drive a
    /// fixed-grid heatmap.
    pub minute_buckets: Vec<MinuteBucket>,
}

/// One row of the replay timeline. Lighter than [`ProvenanceEvent`] —
/// no context windows are carried; the snapshot endpoint slices out a
/// rolling list of these on demand.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayEvent {
    /// ISO 8601 UTC timestamp of the event (RFC 3339).
    pub ts: String,
    pub agent: String,
    /// Canonicalised tool name (e.g. `Edit`, `Read`, `Bash`). Empty
    /// when the row's `tool_canonical` column is null.
    pub tool: String,
    /// Tool family (`Write`, `Read`, `Shell`, …). Empty when the row's
    /// `tool_class` column is null.
    pub tool_class: String,
    pub detail: Option<String>,
}

/// One cell of the 24-hour minute heatmap.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct MinuteBucket {
    /// `0..=1439`. Minute zero is local-midnight on the anchor date.
    pub minute: u16,
    pub count: u16,
}

/// Live state at a specific instant during replay. Returned by the
/// `/api/replay/:date/at?ts=…` route as the scrubber moves; the
/// frontend coalesces requests so at most one is in flight.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReplaySnapshot {
    /// Echoed back so the frontend can sanity-check the response
    /// matches the most recently requested ts (older replies are
    /// dropped).
    pub at: String,
    /// Most recent file path written or edited within the previous
    /// minute, if any. `None` when no write-class event happened in
    /// that window.
    pub active_file: Option<String>,
    /// Up to five most recent events at or before `at`, oldest first.
    pub last_n_events: Vec<ReplayEvent>,
    /// Per-agent call counts in the trailing 60 seconds.
    pub agent_minute_mix: Vec<AgentCount>,
    /// Per-tool call counts in the trailing 60 seconds.
    pub tool_minute_mix: Vec<ToolMixEntry>,
}

/// Aggregate USD-cost view of a window. Emitted by the cost overlay
/// (Phase 3 M6) on every report surface that carries call totals.
///
/// `total_usd` is the sum of every per-agent / per-model bucket. The
/// figure is best-effort by design — see [`crate::cost`]:
///
///   - MCP traffic in the `events` table contributes real tokens (and
///     real model ids) parsed out of each Anthropic-shaped `usage`
///     block.
///   - `agent_events` rows contribute heuristic tokens computed from
///     `len(detail)`. Their per-row [`AgentCost::estimate`] /
///     [`ModelCost::estimate`] flag is `true`.
///
/// `estimate_share` is the dollar fraction (or token fraction when no
/// dollars resolve) attributable to the heuristic bucket. UI surfaces
/// should render an "estimate" footnote when `estimate_share > 0`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CostSummary {
    /// Inclusive ISO-8601 UTC start of the window. Echoed so the
    /// frontend can label the figure without recomputing the bounds.
    pub from: String,
    /// Exclusive ISO-8601 UTC end of the window.
    pub to: String,
    /// Sum of every bucket, rounded to 4 decimal places.
    pub total_usd: f64,
    /// Per-agent breakdown sorted by descending USD.
    pub by_agent: Vec<AgentCost>,
    /// Per-model breakdown sorted by descending USD. The `unknown`
    /// model bucket holds estimated tokens whose source agent has no
    /// default model mapping.
    pub by_model: Vec<ModelCost>,
    /// Heuristic-bucket fraction, clamped to `[0.0, 1.0]`.
    pub estimate_share: f64,
}

/// One row of [`CostSummary::by_agent`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCost {
    pub agent: String,
    pub usd: f64,
    pub tokens_in: u64,
    pub tokens_out: u64,
    /// `true` when every token in the bucket came from the heuristic
    /// path (no MCP usage parsed for this agent in the window).
    pub estimate: bool,
}

/// One row of [`CostSummary::by_model`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCost {
    pub model: String,
    pub usd: f64,
    pub tokens_in: u64,
    pub tokens_out: u64,
    /// `true` when every token in the bucket came from the heuristic
    /// path (no MCP usage parsed for this model in the window).
    pub estimate: bool,
}
