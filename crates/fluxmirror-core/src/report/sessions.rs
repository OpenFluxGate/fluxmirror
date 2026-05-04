// Phase 3 M5 — heuristic session inference.
//
// Cluster `agent_events` rows into "work sessions", classify the
// lifecycle of each (Shipping / Building / Polishing / Testing /
// Investigating / Idle), and synthesise a deterministic, self-
// descriptive name. No model, no network, no language pack — pure
// rule-based logic so the same inputs always produce the same output.
//
// Two public entrypoints feed the studio API:
//   * `collect_sessions(conn, tz, range)` — list view, events elided.
//   * `collect_session_detail(conn, id)`  — single session by id with
//     the per-event timeline populated.
//
// Both share `cluster_events` + `keep_cluster` so an id surfaced on
// the list page resolves to exactly one session on the detail page.

use std::collections::{BTreeSet, HashMap};

use chrono::{DateTime, Duration, Utc};
use chrono_tz::Tz;
use rusqlite::Connection;

use super::data::{is_read, is_shell, is_write};
use super::dto::{Session, SessionEvent, SessionLifecycle, ToolMixEntry, WindowRange};

/// Maximum gap between two consecutive events that still keeps them in
/// the same session. Anything larger and we open a fresh session.
const SESSION_GAP_MINUTES: i64 = 30;

/// Sessions strictly shorter than this AND with strictly fewer than
/// [`MIN_EVENT_COUNT`] events are dropped — they're noise from a
/// single-tab open or a dropped capture.
const MIN_DURATION_MINUTES: i64 = 5;
const MIN_EVENT_COUNT: usize = 5;

/// Hard cap on `top_files` in the DTO.
const TOP_FILES_LIMIT: usize = 5;

/// Window the detail endpoint scans before re-clustering. 30 days is
/// well under the SQLite size pressure from a single-user capture and
/// keeps the linear scan fast.
const SESSION_DETAIL_DAYS: i64 = 30;

/// Minimum identical test-cycle count needed to flip a session to
/// Testing.
const TESTING_MIN_CYCLES: u64 = 5;

/// Minimum distinct files touched needed to flip an Edit-heavy session
/// from Building (focused) to Polishing (cleanup pass).
const POLISHING_MIN_FILES: usize = 3;

/// FNV-1a 64-bit. Picked over `DefaultHasher` because SipHash's keyed
/// init isn't stable across Rust releases — we want session ids that
/// stay valid across recompilations.
const FNV_OFFSET: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;

/// One row pulled from `agent_events` for clustering. Kept as plain
/// fields (no Arc/Rc) because a 30-day window is at worst tens of
/// thousands of rows and the cost is dominated by SQLite I/O.
#[derive(Debug, Clone)]
struct RawEvent {
    ts: DateTime<Utc>,
    /// Original RFC3339 string from the row, preserved so the DTO
    /// echoes the same string the rest of the API uses.
    ts_iso: String,
    agent: String,
    tool: String,
    detail: String,
    cwd: String,
}

/// Pre-computed signal counts for a single cluster. Kept on the side
/// so both the lifecycle classifier and the name generator read from
/// the same numbers (cheaper + ensures classifier and name agree).
#[derive(Debug, Default)]
pub(crate) struct ClusterStats {
    edit_count: u64,
    read_count: u64,
    bash_count: u64,
    distinct_files: BTreeSet<String>,
    distinct_cwds: BTreeSet<String>,
    /// Highest count seen for any single canonical test command (e.g.
    /// `cargo test`). Drives the Testing lifecycle threshold.
    test_cycles_max: u64,
    /// Canonical name of the test command that produced
    /// [`Self::test_cycles_max`]. Used in the name's tail signature.
    test_family_top: Option<String>,
    has_git_tag: bool,
    has_git_push: bool,
    /// First tag-name argument seen on a `git tag …` shell call.
    /// Powers the `(shipped <tag>)` tail in the name.
    git_tag_name: Option<String>,
}

/// Pull every `agent_events` row inside `[start_utc, end_utc)`, cluster
/// the rows into work sessions, and return them with events elided.
pub fn collect_sessions(
    conn: &Connection,
    _tz: &Tz,
    range: WindowRange,
) -> Result<Vec<Session>, String> {
    let raw = fetch_events(conn, range.start_utc, range.end_utc)?;
    let clusters = cluster_events(&raw);
    let mut out: Vec<Session> = Vec::new();
    for cluster in clusters {
        if !keep_cluster(&cluster) {
            continue;
        }
        out.push(build_session(&cluster, false));
    }
    Ok(out)
}

/// Re-derive every session over the trailing 30 days and return the
/// one whose id matches. `Ok(None)` means the caller should 404.
pub fn collect_session_detail(conn: &Connection, id: &str) -> Result<Option<Session>, String> {
    let now = Utc::now();
    let start = now - Duration::days(SESSION_DETAIL_DAYS);
    // Add a one-second pad so an event captured right at request time
    // is still inside the half-open window.
    let end = now + Duration::seconds(1);
    let raw = fetch_events(conn, start, end)?;
    let clusters = cluster_events(&raw);
    for cluster in clusters {
        if !keep_cluster(&cluster) {
            continue;
        }
        let session = build_session(&cluster, true);
        if session.id == id {
            return Ok(Some(session));
        }
    }
    Ok(None)
}

/// Pull rows from `agent_events` ordered by ts ascending. Skips rows
/// with unparseable `ts` rather than aborting the whole request.
fn fetch_events(
    conn: &Connection,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Result<Vec<RawEvent>, String> {
    let start_str = start.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let end_str = end.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let mut stmt = conn
        .prepare(
            "SELECT ts, agent, COALESCE(tool, '') AS tool, \
                    COALESCE(detail, '') AS detail, COALESCE(cwd, '') AS cwd \
             FROM agent_events WHERE ts >= ?1 AND ts < ?2 ORDER BY ts ASC, id ASC",
        )
        .map_err(|e| format!("prepare(sessions): {e}"))?;
    let mapped = stmt
        .query_map([&start_str, &end_str], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
            ))
        })
        .map_err(|e| format!("query(sessions): {e}"))?;
    let mut out = Vec::new();
    for row in mapped {
        let (ts_str, agent, tool, detail, cwd) =
            row.map_err(|e| format!("row(sessions): {e}"))?;
        let ts = match DateTime::parse_from_rfc3339(&ts_str) {
            Ok(dt) => dt.with_timezone(&Utc),
            Err(_) => continue,
        };
        out.push(RawEvent {
            ts,
            ts_iso: ts_str,
            agent,
            tool,
            detail,
            cwd,
        });
    }
    Ok(out)
}

/// Walk a sorted event stream, opening a fresh cluster every time the
/// gap to the previous event exceeds [`SESSION_GAP_MINUTES`].
fn cluster_events(events: &[RawEvent]) -> Vec<Vec<RawEvent>> {
    if events.is_empty() {
        return Vec::new();
    }
    let gap = Duration::minutes(SESSION_GAP_MINUTES);
    let mut out: Vec<Vec<RawEvent>> = Vec::new();
    let mut current: Vec<RawEvent> = Vec::new();
    let mut prev_ts: Option<DateTime<Utc>> = None;
    for ev in events {
        let split = match prev_ts {
            Some(p) => ev.ts - p > gap,
            None => false,
        };
        if split && !current.is_empty() {
            out.push(std::mem::take(&mut current));
        }
        prev_ts = Some(ev.ts);
        current.push(ev.clone());
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// Drop noise sessions: shorter than 5 minutes AND fewer than 5 events.
/// Either condition alone keeps the cluster — a one-event-on-a-Sunday
/// quick check earns its row, and so does a 4-event 12-minute hop.
fn keep_cluster(cluster: &[RawEvent]) -> bool {
    let count = cluster.len();
    let duration = if count >= 2 {
        (cluster[count - 1].ts - cluster[0].ts).num_minutes()
    } else {
        0
    };
    !(duration < MIN_DURATION_MINUTES && count < MIN_EVENT_COUNT)
}

fn build_session(cluster: &[RawEvent], include_events: bool) -> Session {
    let stats = analyze_cluster(cluster);
    let lifecycle = classify_lifecycle(&stats);

    let event_count = cluster.len() as i64;
    let start_dt = cluster[0].ts;
    let end_dt = cluster[cluster.len() - 1].ts;
    let start = start_dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let end = end_dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

    let mut agents_set: BTreeSet<String> = BTreeSet::new();
    for e in cluster {
        agents_set.insert(e.agent.clone());
    }
    let agents: Vec<String> = agents_set.into_iter().collect();

    let mut cwd_counts: HashMap<String, u64> = HashMap::new();
    for e in cluster {
        if !e.cwd.is_empty() {
            *cwd_counts.entry(e.cwd.clone()).or_default() += 1;
        }
    }
    let dominant_cwd = cwd_counts
        .iter()
        .max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0)))
        .map(|(k, _)| k.clone());

    let mut file_counts: HashMap<String, u64> = HashMap::new();
    for e in cluster {
        if is_write(&e.tool) && !e.detail.is_empty() {
            *file_counts.entry(e.detail.clone()).or_default() += 1;
        }
    }
    let mut file_sorted: Vec<(String, u64)> = file_counts.into_iter().collect();
    file_sorted.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let top_files: Vec<String> = file_sorted
        .into_iter()
        .take(TOP_FILES_LIMIT)
        .map(|(p, _)| p)
        .collect();

    let mut tool_counts: HashMap<String, u64> = HashMap::new();
    for e in cluster {
        if !e.tool.is_empty() {
            *tool_counts.entry(e.tool.clone()).or_default() += 1;
        }
    }
    let mut tool_mix: Vec<ToolMixEntry> = tool_counts
        .into_iter()
        .map(|(t, n)| ToolMixEntry { tool: t, count: n })
        .collect();
    tool_mix.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.tool.cmp(&b.tool)));

    let id = session_id(&start, &end);

    let events_dto: Vec<SessionEvent> = if include_events {
        cluster
            .iter()
            .map(|e| SessionEvent {
                ts: e.ts_iso.clone(),
                agent: e.agent.clone(),
                tool: e.tool.clone(),
                detail: if e.detail.is_empty() {
                    None
                } else {
                    Some(e.detail.clone())
                },
            })
            .collect()
    } else {
        Vec::new()
    };

    let mut session = Session {
        id,
        start,
        end,
        agents,
        event_count,
        dominant_cwd,
        top_files,
        tool_mix,
        lifecycle,
        name: String::new(),
        intent: None,
        events: events_dto,
    };
    session.name = generate_name(&session, &stats);
    session
}

/// Walk every event once and tally signals the classifier and name
/// generator both need.
fn analyze_cluster(cluster: &[RawEvent]) -> ClusterStats {
    let mut stats = ClusterStats::default();
    let mut test_cycles: HashMap<String, u64> = HashMap::new();
    for e in cluster {
        let tool = e.tool.as_str();
        if !e.cwd.is_empty() {
            stats.distinct_cwds.insert(e.cwd.clone());
        }
        if is_write(tool) {
            stats.edit_count += 1;
            if !e.detail.is_empty() {
                stats.distinct_files.insert(e.detail.clone());
            }
        } else if is_read(tool) {
            stats.read_count += 1;
        } else if is_shell(tool) {
            stats.bash_count += 1;
            if !e.detail.is_empty() {
                let trimmed = e.detail.trim();
                if trimmed.starts_with("git tag") {
                    stats.has_git_tag = true;
                    if stats.git_tag_name.is_none() {
                        stats.git_tag_name = extract_tag_name(trimmed);
                    }
                }
                if trimmed.starts_with("git push") {
                    stats.has_git_push = true;
                }
                if let Some(family) = canonicalize_test_cmd(trimmed) {
                    *test_cycles.entry(family).or_default() += 1;
                }
            }
        }
    }
    if let Some((family, count)) = test_cycles
        .iter()
        .max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0)))
    {
        stats.test_cycles_max = *count;
        stats.test_family_top = Some(family.clone());
    }
    stats
}

/// Map a shell command to its test-runner family, or `None` if it
/// doesn't look like a recognised test command.
fn canonicalize_test_cmd(cmd: &str) -> Option<String> {
    let trimmed = cmd.trim_start();
    if trimmed.starts_with("cargo test") {
        return Some("cargo".to_string());
    }
    if trimmed.starts_with("pytest") || trimmed.starts_with("python -m pytest") {
        return Some("pytest".to_string());
    }
    if trimmed.starts_with("npm test") || trimmed.starts_with("npm run test") {
        return Some("npm".to_string());
    }
    if trimmed.starts_with("go test") {
        return Some("go".to_string());
    }
    None
}

/// Pull the first tag-name argument off a `git tag …` invocation,
/// skipping flag tokens (and their values for `-m`/`-u`/`--cleanup`).
pub(crate) fn extract_tag_name(cmd: &str) -> Option<String> {
    let after = cmd.trim_start().strip_prefix("git tag")?.trim_start();
    if after.is_empty() {
        return None;
    }
    let tokens: Vec<&str> = after.split_whitespace().collect();
    let mut i = 0;
    while i < tokens.len() {
        let tok = tokens[i];
        // Single-char flags that DO NOT take a separate argument.
        if matches!(tok, "-a" | "-s" | "-f" | "-d" | "-l" | "-n") {
            i += 1;
            continue;
        }
        // Single-char flags that DO take a separate argument.
        if matches!(tok, "-m" | "-u" | "--cleanup") {
            i += 2;
            continue;
        }
        // Any other long flag — skip it alone.
        if tok.starts_with('-') {
            i += 1;
            continue;
        }
        return Some(tok.trim_matches('"').trim_matches('\'').to_string());
    }
    None
}

/// Pure classifier — order matters, first match wins. Crate-private
/// because [`ClusterStats`] is the natural input shape and we don't
/// want to commit to that across the crate boundary.
pub(crate) fn classify_lifecycle(stats: &ClusterStats) -> SessionLifecycle {
    if stats.has_git_tag && stats.has_git_push {
        return SessionLifecycle::Shipping;
    }
    if stats.test_cycles_max >= TESTING_MIN_CYCLES {
        return SessionLifecycle::Testing;
    }
    let edit_threshold = stats.read_count as f64 * 0.5;
    if stats.edit_count > 0 && (stats.edit_count as f64) > edit_threshold {
        if stats.distinct_files.len() <= POLISHING_MIN_FILES {
            return SessionLifecycle::Building;
        }
        return SessionLifecycle::Polishing;
    }
    if stats.read_count > stats.edit_count.saturating_mul(2) && stats.bash_count > 0 {
        return SessionLifecycle::Investigating;
    }
    SessionLifecycle::Idle
}

/// FNV-1a 64-bit, truncated to the leading 32 bits, formatted as 8
/// lowercase hex chars.
pub fn session_id(start_iso: &str, end_iso: &str) -> String {
    let mut hash = FNV_OFFSET;
    for byte in start_iso.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash ^= u64::from(b'|');
    hash = hash.wrapping_mul(FNV_PRIME);
    for byte in end_iso.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    format!("{:08x}", (hash >> 32) as u32)
}

/// Synthesise the session name. Format: `<Verb>: <Object> (<Tail>)`.
pub(crate) fn generate_name(session: &Session, stats: &ClusterStats) -> String {
    let verb = lifecycle_verb(session.lifecycle);
    let object = derive_object(session);
    let tail = derive_tail(session, stats);
    if tail.is_empty() {
        format!("{verb}: {object}")
    } else {
        format!("{verb}: {object} ({tail})")
    }
}

fn lifecycle_verb(lifecycle: SessionLifecycle) -> &'static str {
    match lifecycle {
        SessionLifecycle::Shipping => "Shipped",
        SessionLifecycle::Testing => "Tested",
        SessionLifecycle::Building => "Built",
        SessionLifecycle::Polishing => "Polished",
        SessionLifecycle::Investigating => "Investigated",
        SessionLifecycle::Idle => "Idle",
    }
}

/// Object phrase: dominant cwd's last 3 path segments (or top file's
/// parent directory when the cwd is essentially the repo root).
fn derive_object(session: &Session) -> String {
    if let Some(cwd) = &session.dominant_cwd {
        let parts: Vec<&str> = cwd
            .split('/')
            .filter(|p| !p.is_empty())
            .collect();
        if parts.len() >= 3 {
            let start = parts.len() - 3;
            return parts[start..].join("/");
        }
        // 1-2 segments → use top file's parent directory if available,
        // else fall back to the cwd itself.
        if let Some(file) = session.top_files.first() {
            let fparts: Vec<&str> = file
                .split('/')
                .filter(|p| !p.is_empty())
                .collect();
            if fparts.len() >= 2 {
                let take = fparts.len().min(3);
                let start = fparts.len() - take;
                let end = fparts.len() - 1;
                if start < end {
                    return fparts[start..end].join("/");
                }
            }
        }
        if !parts.is_empty() {
            return parts.join("/");
        }
    }
    if let Some(file) = session.top_files.first() {
        let fparts: Vec<&str> = file
            .split('/')
            .filter(|p| !p.is_empty())
            .collect();
        if fparts.len() >= 2 {
            let take = fparts.len().min(3);
            let start = fparts.len() - take;
            let end = fparts.len() - 1;
            if start < end {
                return fparts[start..end].join("/");
            }
        }
        return file.clone();
    }
    "—".to_string()
}

fn derive_tail(session: &Session, stats: &ClusterStats) -> String {
    match session.lifecycle {
        SessionLifecycle::Testing => {
            let family = stats
                .test_family_top
                .as_deref()
                .unwrap_or("test");
            format!(
                "{} {} cycles, {} edits",
                stats.test_cycles_max, family, stats.edit_count
            )
        }
        SessionLifecycle::Building | SessionLifecycle::Polishing => {
            format!("Edit-heavy, {} files", stats.distinct_files.len())
        }
        SessionLifecycle::Investigating => {
            format!("Read-heavy, {} cwds", stats.distinct_cwds.len())
        }
        SessionLifecycle::Shipping => {
            if let Some(tag) = &stats.git_tag_name {
                format!("shipped {tag}")
            } else {
                "shipped".to_string()
            }
        }
        SessionLifecycle::Idle => format!("{} events", session.event_count),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn make_event(ts: &str, agent: &str, tool: &str, detail: &str, cwd: &str) -> RawEvent {
        let dt = DateTime::parse_from_rfc3339(ts).unwrap().with_timezone(&Utc);
        RawEvent {
            ts: dt,
            ts_iso: ts.to_string(),
            agent: agent.to_string(),
            tool: tool.to_string(),
            detail: detail.to_string(),
            cwd: cwd.to_string(),
        }
    }

    fn iso(year: i32, mo: u32, day: u32, h: u32, m: u32) -> String {
        let dt = Utc.with_ymd_and_hms(year, mo, day, h, m, 0).unwrap();
        dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
    }

    fn build_cluster_burst(
        start_minute: u32,
        count: usize,
        tool: &str,
        detail_prefix: &str,
        cwd: &str,
    ) -> Vec<RawEvent> {
        (0..count)
            .map(|i| {
                let ts = iso(2026, 4, 26, 10, start_minute + i as u32);
                make_event(
                    &ts,
                    "claude-code",
                    tool,
                    &format!("{}{}", detail_prefix, i),
                    cwd,
                )
            })
            .collect()
    }

    #[test]
    fn cluster_events_empty_returns_empty() {
        let out = cluster_events(&[]);
        assert!(out.is_empty());
    }

    #[test]
    fn cluster_events_single_cluster_with_small_gaps() {
        let events = build_cluster_burst(0, 6, "Edit", "src/foo", "/proj");
        let clusters = cluster_events(&events);
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].len(), 6);
    }

    #[test]
    fn cluster_events_splits_on_30min_gap() {
        let mut events = build_cluster_burst(0, 4, "Edit", "src/a", "/proj");
        // 31-minute gap before the next batch — opens a new session.
        let mut tail = build_cluster_burst(35, 5, "Read", "src/b", "/proj");
        events.append(&mut tail);
        let clusters = cluster_events(&events);
        assert_eq!(clusters.len(), 2);
        assert_eq!(clusters[0].len(), 4);
        assert_eq!(clusters[1].len(), 5);
    }

    #[test]
    fn cluster_events_keeps_at_30min_boundary() {
        // Exactly 30 minutes is NOT > 30 minutes — same session.
        let a = make_event("2026-04-26T10:00:00Z", "claude-code", "Edit", "x", "/p");
        let b = make_event("2026-04-26T10:30:00Z", "claude-code", "Edit", "y", "/p");
        let clusters = cluster_events(&[a, b]);
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].len(), 2);
    }

    #[test]
    fn keep_cluster_drops_short_and_small() {
        // 1 event at duration 0 — drop.
        let a = vec![make_event(
            "2026-04-26T10:00:00Z",
            "claude-code",
            "Edit",
            "x",
            "/p",
        )];
        assert!(!keep_cluster(&a));

        // 4 events spanning 4 minutes — drop.
        let b = (0..4)
            .map(|i| {
                let ts = iso(2026, 4, 26, 10, i);
                make_event(&ts, "claude-code", "Edit", "x", "/p")
            })
            .collect::<Vec<_>>();
        assert!(!keep_cluster(&b));
    }

    #[test]
    fn keep_cluster_keeps_short_but_busy() {
        // 5 events spanning 3 minutes — duration is short but the
        // event_count threshold is met, so keep.
        let events: Vec<RawEvent> = (0..5)
            .map(|i| {
                let ts = iso(2026, 4, 26, 10, i);
                make_event(&ts, "claude-code", "Edit", &format!("x{i}"), "/p")
            })
            .collect();
        assert!(keep_cluster(&events));
    }

    #[test]
    fn keep_cluster_keeps_long_but_quiet() {
        // 2 events spanning 10 minutes — count is small but duration
        // crosses the floor, so keep.
        let events = vec![
            make_event("2026-04-26T10:00:00Z", "claude-code", "Edit", "a", "/p"),
            make_event("2026-04-26T10:10:00Z", "claude-code", "Edit", "b", "/p"),
        ];
        assert!(keep_cluster(&events));
    }

    #[test]
    fn classify_lifecycle_shipping_requires_tag_and_push() {
        // Only git tag — not Shipping.
        let only_tag = ClusterStats {
            has_git_tag: true,
            ..Default::default()
        };
        assert_ne!(classify_lifecycle(&only_tag), SessionLifecycle::Shipping);

        // Tag + push — Shipping.
        let both = ClusterStats {
            has_git_tag: true,
            has_git_push: true,
            ..Default::default()
        };
        assert_eq!(classify_lifecycle(&both), SessionLifecycle::Shipping);
    }

    #[test]
    fn classify_lifecycle_testing_requires_5_cycles() {
        let four = ClusterStats {
            test_cycles_max: 4,
            test_family_top: Some("cargo".into()),
            ..Default::default()
        };
        assert_ne!(classify_lifecycle(&four), SessionLifecycle::Testing);

        let five = ClusterStats {
            test_cycles_max: 5,
            test_family_top: Some("cargo".into()),
            ..Default::default()
        };
        assert_eq!(classify_lifecycle(&five), SessionLifecycle::Testing);
    }

    #[test]
    fn classify_lifecycle_building_when_focused_edits() {
        let mut s = ClusterStats {
            edit_count: 8,
            read_count: 2,
            ..Default::default()
        };
        s.distinct_files.insert("src/a.rs".into());
        s.distinct_files.insert("src/b.rs".into());
        assert_eq!(classify_lifecycle(&s), SessionLifecycle::Building);
    }

    #[test]
    fn classify_lifecycle_polishing_when_many_files() {
        let mut s = ClusterStats {
            edit_count: 20,
            read_count: 3,
            ..Default::default()
        };
        for i in 0..10 {
            s.distinct_files.insert(format!("src/{i}.rs"));
        }
        assert_eq!(classify_lifecycle(&s), SessionLifecycle::Polishing);
    }

    #[test]
    fn classify_lifecycle_investigating_when_read_heavy() {
        let s = ClusterStats {
            edit_count: 1,
            read_count: 10,
            bash_count: 3,
            ..Default::default()
        };
        assert_eq!(classify_lifecycle(&s), SessionLifecycle::Investigating);
    }

    #[test]
    fn classify_lifecycle_investigating_needs_bash() {
        // Read-heavy but zero Bash → Idle (the rule explicitly demands
        // Bash > 0 to call it an investigation).
        let s = ClusterStats {
            edit_count: 0,
            read_count: 8,
            bash_count: 0,
            ..Default::default()
        };
        assert_eq!(classify_lifecycle(&s), SessionLifecycle::Idle);
    }

    #[test]
    fn classify_lifecycle_idle_default() {
        let s = ClusterStats::default();
        assert_eq!(classify_lifecycle(&s), SessionLifecycle::Idle);
    }

    #[test]
    fn session_id_is_deterministic_and_8_hex() {
        let a = session_id("2026-04-26T10:00:00Z", "2026-04-26T11:00:00Z");
        let b = session_id("2026-04-26T10:00:00Z", "2026-04-26T11:00:00Z");
        assert_eq!(a, b);
        assert_eq!(a.len(), 8);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit() && !c.is_uppercase()));
    }

    #[test]
    fn session_id_changes_with_inputs() {
        let a = session_id("2026-04-26T10:00:00Z", "2026-04-26T11:00:00Z");
        let b = session_id("2026-04-26T10:00:00Z", "2026-04-26T11:00:01Z");
        assert_ne!(a, b);
    }

    #[test]
    fn extract_tag_name_simple() {
        assert_eq!(
            extract_tag_name("git tag v0.6.0"),
            Some("v0.6.0".to_string())
        );
    }

    #[test]
    fn extract_tag_name_skips_flag_args() {
        assert_eq!(
            extract_tag_name("git tag -a v0.6.0 -m 'release'"),
            Some("v0.6.0".to_string())
        );
        assert_eq!(
            extract_tag_name("git tag -m 'msg' v0.6.0"),
            Some("v0.6.0".to_string())
        );
    }

    #[test]
    fn extract_tag_name_handles_quotes() {
        assert_eq!(
            extract_tag_name("git tag \"v1.2.3\""),
            Some("v1.2.3".to_string())
        );
    }

    #[test]
    fn canonicalize_test_cmd_recognises_runners() {
        assert_eq!(
            canonicalize_test_cmd("cargo test --workspace"),
            Some("cargo".to_string())
        );
        assert_eq!(
            canonicalize_test_cmd("pytest -k foo"),
            Some("pytest".to_string())
        );
        assert_eq!(
            canonicalize_test_cmd("npm test"),
            Some("npm".to_string())
        );
        assert_eq!(
            canonicalize_test_cmd("npm run test"),
            Some("npm".to_string())
        );
        assert_eq!(
            canonicalize_test_cmd("go test ./..."),
            Some("go".to_string())
        );
        assert_eq!(canonicalize_test_cmd("ls"), None);
    }

    #[test]
    fn build_session_naming_format_for_each_lifecycle() {
        // Building — focused edits.
        let edits = (0..6)
            .map(|i| {
                let ts = iso(2026, 4, 26, 10, i);
                make_event(
                    &ts,
                    "claude-code",
                    "Edit",
                    "crates/fluxmirror-core/src/lib.rs",
                    "/Users/me/proj/fluxmirror",
                )
            })
            .collect::<Vec<_>>();
        let session = build_session(&edits, false);
        assert_eq!(session.lifecycle, SessionLifecycle::Building);
        assert!(
            session.name.starts_with("Built: "),
            "got: {}",
            session.name
        );
        assert!(session.name.contains("Edit-heavy"), "got: {}", session.name);

        // Investigating — read-heavy.
        let mut investigation: Vec<RawEvent> = Vec::new();
        for i in 0..8 {
            let ts = iso(2026, 4, 26, 11, i);
            investigation.push(make_event(
                &ts,
                "claude-code",
                "Read",
                &format!("src/{}.rs", i),
                "/proj/a",
            ));
        }
        investigation.push(make_event(
            "2026-04-26T11:09:00Z",
            "claude-code",
            "Bash",
            "rg foo",
            "/proj/a",
        ));
        let session = build_session(&investigation, false);
        assert_eq!(session.lifecycle, SessionLifecycle::Investigating);
        assert!(session.name.starts_with("Investigated: "));
        assert!(session.name.contains("Read-heavy"));

        // Shipping.
        let ship = vec![
            make_event(
                "2026-04-26T12:00:00Z",
                "claude-code",
                "Bash",
                "git tag v0.6.0",
                "/proj/a",
            ),
            make_event(
                "2026-04-26T12:01:00Z",
                "claude-code",
                "Bash",
                "git push origin v0.6.0",
                "/proj/a",
            ),
            make_event(
                "2026-04-26T12:05:00Z",
                "claude-code",
                "Bash",
                "git push origin main",
                "/proj/a",
            ),
        ];
        let session = build_session(&ship, false);
        assert_eq!(session.lifecycle, SessionLifecycle::Shipping);
        assert!(session.name.starts_with("Shipped: "));
        assert!(session.name.contains("v0.6.0"), "got: {}", session.name);
    }

    #[test]
    fn build_session_top_files_capped_and_sorted() {
        // Seven distinct files — top_files capped at 5, sorted by count
        // desc, then path asc. file0 gets 3 hits, others 1 each.
        let mut events: Vec<RawEvent> = Vec::new();
        for i in 0..3 {
            let ts = iso(2026, 4, 26, 10, i);
            events.push(make_event(&ts, "claude-code", "Edit", "src/file0.rs", "/p"));
        }
        for i in 1..7 {
            let ts = iso(2026, 4, 26, 10, 3 + i);
            events.push(make_event(
                &ts,
                "claude-code",
                "Edit",
                &format!("src/file{i}.rs"),
                "/p",
            ));
        }
        let session = build_session(&events, false);
        assert_eq!(session.top_files.len(), TOP_FILES_LIMIT);
        assert_eq!(session.top_files[0], "src/file0.rs");
    }

    #[test]
    fn build_session_id_stable_across_runs() {
        let events = build_cluster_burst(0, 6, "Edit", "src/a", "/proj");
        let s1 = build_session(&events, false);
        let s2 = build_session(&events, false);
        assert_eq!(s1.id, s2.id);
        assert_eq!(s1.name, s2.name);
        assert_eq!(s1.start, s2.start);
        assert_eq!(s1.end, s2.end);
    }

    #[test]
    fn build_session_events_only_when_requested() {
        let events = build_cluster_burst(0, 6, "Edit", "src/a", "/proj");
        let with_events = build_session(&events, true);
        assert_eq!(with_events.events.len(), 6);
        let without = build_session(&events, false);
        assert!(without.events.is_empty());
    }

    #[test]
    fn derive_object_uses_top_file_dir_when_cwd_is_short() {
        let mut session = Session {
            id: "x".into(),
            start: "s".into(),
            end: "e".into(),
            agents: vec!["claude-code".into()],
            event_count: 6,
            dominant_cwd: Some("/tmp".into()),
            top_files: vec!["crates/foo/src/lib.rs".into()],
            tool_mix: vec![],
            lifecycle: SessionLifecycle::Building,
            name: String::new(),
            intent: None,
            events: vec![],
        };
        // 1 cwd segment ("tmp") — fall back to top file's parent
        // directory: "crates/foo/src" minus the filename → "foo/src".
        let object = derive_object(&session);
        assert!(object.contains("crates") || object.contains("src"));

        // 3+ cwd segments — last 3 segments win.
        session.dominant_cwd = Some("/Users/me/proj/fluxmirror".into());
        assert_eq!(derive_object(&session), "me/proj/fluxmirror");
    }
}
