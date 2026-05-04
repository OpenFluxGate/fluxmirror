// Phase 4 M-A6 — heuristic anomaly detection.
//
// Five deterministic, side-effect-free rules over `agent_events` plus
// the proxy `events` table. Each rule compares a current-window value
// against a rolling 4-week baseline and yields an `AnomalyDetection`
// when the gap exceeds the rule's threshold.
//
// The studio's API layer wraps each detection with
// `fluxmirror_ai::synthesise_anomaly` to produce a one-sentence story.
// On any LLM failure (provider off, budget hit, network) the wrapper
// substitutes a deterministic template, so the detector below never
// needs to know the AI layer exists.
//
// Output is intentionally compact: never more than 5 evidence strings
// per detection, and the rule list itself is a small fixed sequence.
// The whole pass is cheap enough to re-run on every API request — the
// caching is at the LLM-synthesise layer, not here.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use chrono::{DateTime, Datelike, Duration, TimeZone, Utc};
use chrono_tz::Tz;
use rusqlite::Connection;

use super::data::is_write;
use super::dto::AnomalyKind;
use crate::config::Config;
use crate::cost::{
    cost_for_usage, default_model_for_agent, heuristic_from_detail, lookup, parse_message,
    ParsedUsage,
};

/// Window scope for [`detect_anomalies`]. The `Today` window is the
/// local-tz day anchored on `now`; `Week` is the trailing 7 local-tz
/// days ending at `now`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnomalyWindow {
    Today,
    Week,
}

/// One heuristic detection. Carries the raw figures so the LLM wrapper
/// can produce a story without re-running the SQL pass.
#[derive(Debug, Clone)]
pub struct AnomalyDetection {
    pub kind: AnomalyKind,
    pub observed: f64,
    pub baseline: f64,
    pub evidence: Vec<String>,
}

/// Length of the rolling baseline window in days.
const BASELINE_DAYS: i64 = 28;

/// FileEditSpike: minimum edit count required before the rule fires
/// (avoids false positives on a brand-new file with one edit today).
const FILE_EDIT_MIN: u64 = 5;

/// FileEditSpike: ratio of observed-to-baseline that triggers the rule.
const FILE_EDIT_RATIO: f64 = 3.0;

/// ToolMixDeparture: cosine distance between current and baseline mix
/// vectors that triggers the rule.
const TOOL_MIX_DISTANCE: f64 = 0.4;

/// CostPerCallRise: ratio of observed-to-baseline that triggers the rule.
const COST_RATIO: f64 = 2.0;

/// Maximum number of supporting fact strings per detection. Keeps the
/// LLM context cheap and the UI badge readable.
const EVIDENCE_LIMIT: usize = 5;

/// Public entry. Resolves `now` from `Utc::now()` and the timezone from
/// `config.timezone`. Tests should call [`detect_anomalies_at`] instead
/// so they can pin a deterministic clock.
pub fn detect_anomalies(
    conn: &Connection,
    config: &Config,
    window: AnomalyWindow,
) -> Result<Vec<AnomalyDetection>, String> {
    let tz: Tz = config
        .timezone
        .parse()
        .map_err(|_| format!("invalid timezone in config: {}", config.timezone))?;
    detect_anomalies_at(conn, &tz, window, Utc::now())
}

/// Deterministic-clock entry point. Useful for tests + replays.
pub fn detect_anomalies_at(
    conn: &Connection,
    tz: &Tz,
    window: AnomalyWindow,
    now: DateTime<Utc>,
) -> Result<Vec<AnomalyDetection>, String> {
    let (window_start, window_end) = window_bounds(tz, window, now)?;
    let baseline_start = window_start - Duration::days(BASELINE_DAYS);
    let baseline_end = window_start;

    let window_rows = load_agent_events(conn, window_start, window_end)?;
    let baseline_rows = load_agent_events(conn, baseline_start, baseline_end)?;
    let window_methods = load_mcp_methods(conn, window_start, window_end)?;
    let baseline_methods = load_mcp_methods(conn, baseline_start, baseline_end)?;

    let baseline_days = days_in_range(baseline_start, baseline_end).max(1) as f64;
    let window_days = days_in_range(window_start, window_end).max(1) as f64;

    let mut out = Vec::new();
    if let Some(d) = file_edit_spike(&window_rows, &baseline_rows, baseline_days, window_days) {
        out.push(d);
    }
    if let Some(d) = tool_mix_departure(&window_rows, &baseline_rows) {
        out.push(d);
    }
    if let Some(d) = new_agent(&window_rows, &baseline_rows) {
        out.push(d);
    }
    if let Some(d) = new_mcp_method(&window_methods, &baseline_methods) {
        out.push(d);
    }
    if let Some(d) = cost_per_call_rise(
        conn,
        window_start,
        window_end,
        baseline_start,
        baseline_end,
        baseline_days,
        window_days,
    )? {
        out.push(d);
    }
    Ok(out)
}

/// Resolve the local-day or local-week UTC bounds anchored on `now`.
fn window_bounds(
    tz: &Tz,
    window: AnomalyWindow,
    now: DateTime<Utc>,
) -> Result<(DateTime<Utc>, DateTime<Utc>), String> {
    let now_local = now.with_timezone(tz);
    let date = now_local.date_naive();
    let next = date + Duration::days(1);
    let day_start_local = tz
        .with_ymd_and_hms(date.year(), date.month(), date.day(), 0, 0, 0)
        .single()
        .ok_or_else(|| format!("cannot resolve local midnight for {date} in {tz}"))?;
    let day_end_local = tz
        .with_ymd_and_hms(next.year(), next.month(), next.day(), 0, 0, 0)
        .single()
        .ok_or_else(|| format!("cannot resolve local midnight for {next} in {tz}"))?;
    let day_start = day_start_local.with_timezone(&Utc);
    let day_end = day_end_local.with_timezone(&Utc);
    let bounds = match window {
        AnomalyWindow::Today => (day_start, day_end),
        AnomalyWindow::Week => (day_end - Duration::days(7), day_end),
    };
    Ok(bounds)
}

#[derive(Debug, Clone)]
struct AgentRow {
    agent: String,
    tool: String,
    detail: String,
}

fn load_agent_events(
    conn: &Connection,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Result<Vec<AgentRow>, String> {
    let start_str = start.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let end_str = end.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let mut stmt = conn
        .prepare(
            "SELECT agent, COALESCE(tool, '') AS tool, COALESCE(detail, '') AS detail \
             FROM agent_events WHERE ts >= ?1 AND ts < ?2",
        )
        .map_err(|e| format!("prepare(anomaly events): {e}"))?;
    let mapped = stmt
        .query_map([&start_str, &end_str], |r| {
            Ok(AgentRow {
                agent: r.get::<_, String>(0)?,
                tool: r.get::<_, String>(1)?,
                detail: r.get::<_, String>(2)?,
            })
        })
        .map_err(|e| format!("query(anomaly events): {e}"))?;
    let mut out = Vec::new();
    for row in mapped {
        out.push(row.map_err(|e| format!("row(anomaly events): {e}"))?);
    }
    Ok(out)
}

fn load_mcp_methods(
    conn: &Connection,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Result<HashMap<String, u64>, String> {
    let start_ms = start.timestamp_millis();
    let end_ms = end.timestamp_millis();
    let mut stmt = match conn.prepare(
        "SELECT method FROM events \
         WHERE ts_ms >= ?1 AND ts_ms < ?2 AND method IS NOT NULL",
    ) {
        Ok(s) => s,
        Err(_) => return Ok(HashMap::new()),
    };
    let mapped = stmt
        .query_map([&start_ms, &end_ms], |r| r.get::<_, String>(0))
        .map_err(|e| format!("query(anomaly methods): {e}"))?;
    let mut out: HashMap<String, u64> = HashMap::new();
    for row in mapped.flatten() {
        if !row.is_empty() {
            *out.entry(row).or_default() += 1;
        }
    }
    Ok(out)
}

/// FileEditSpike: any single file's edit count in window > 3× rolling
/// 4-week per-day mean (with min count 5 to suppress noise on a fresh file).
fn file_edit_spike(
    window: &[AgentRow],
    baseline: &[AgentRow],
    baseline_days: f64,
    window_days: f64,
) -> Option<AnomalyDetection> {
    let mut window_counts: HashMap<String, u64> = HashMap::new();
    for r in window {
        if is_write(r.tool.as_str()) && !r.detail.is_empty() {
            *window_counts.entry(r.detail.clone()).or_default() += 1;
        }
    }
    let mut baseline_counts: HashMap<String, u64> = HashMap::new();
    for r in baseline {
        if is_write(r.tool.as_str()) && !r.detail.is_empty() {
            *baseline_counts.entry(r.detail.clone()).or_default() += 1;
        }
    }

    // Per-day average for each file in the baseline. Comparison is
    // observed (per window-day) vs baseline (per baseline-day). For a
    // 1-day window the observed rate is the raw count.
    let mut hits: Vec<(String, f64, f64)> = Vec::new();
    for (path, count) in &window_counts {
        if *count < FILE_EDIT_MIN {
            continue;
        }
        let baseline_avg = baseline_counts
            .get(path)
            .copied()
            .map(|c| c as f64 / baseline_days)
            .unwrap_or(0.0);
        // Avoid divide-by-zero: a brand-new file always trips when the
        // observed count clears `FILE_EDIT_MIN`.
        let observed_per_day = *count as f64 / window_days;
        let trips = if baseline_avg > 0.0 {
            observed_per_day >= FILE_EDIT_RATIO * baseline_avg
        } else {
            true
        };
        if trips {
            hits.push((path.clone(), observed_per_day, baseline_avg));
        }
    }
    if hits.is_empty() {
        return None;
    }

    // Sort by observed desc, tiebreak on path asc; top hit drives the
    // headline figures, the rest fill the evidence strip.
    hits.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal)
        .then_with(|| a.0.cmp(&b.0)));
    let top = hits.first().cloned()?;
    let evidence: Vec<String> = hits
        .iter()
        .take(EVIDENCE_LIMIT)
        .map(|(path, obs, base)| format!("{path} +{obs:.0} edits (rolling avg {base:.1})"))
        .collect();
    Some(AnomalyDetection {
        kind: AnomalyKind::FileEditSpike,
        observed: top.1,
        baseline: top.2,
        evidence,
    })
}

/// ToolMixDeparture: cosine distance between window's tool mix and the
/// baseline's tool mix ≥ 0.4. Both vectors are L2-normalised counts
/// over the same tool key universe.
fn tool_mix_departure(
    window: &[AgentRow],
    baseline: &[AgentRow],
) -> Option<AnomalyDetection> {
    let window_mix = tool_mix(window);
    let baseline_mix = tool_mix(baseline);
    if window_mix.is_empty() || baseline_mix.is_empty() {
        return None;
    }
    let distance = cosine_distance(&window_mix, &baseline_mix);
    if distance < TOOL_MIX_DISTANCE {
        return None;
    }

    let mut window_top: Vec<(String, u64)> = window_mix.into_iter().collect();
    window_top.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let mut baseline_top: Vec<(String, u64)> = baseline_mix.into_iter().collect();
    baseline_top.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    let mut evidence: Vec<String> = Vec::new();
    if let Some((tool, n)) = window_top.first() {
        evidence.push(format!("window top tool: {tool} ({n})"));
    }
    if let Some((tool, n)) = baseline_top.first() {
        evidence.push(format!("baseline top tool: {tool} ({n})"));
    }
    evidence.push(format!("cosine distance {distance:.2}"));
    Some(AnomalyDetection {
        kind: AnomalyKind::ToolMixDeparture,
        observed: distance,
        baseline: TOOL_MIX_DISTANCE,
        evidence,
    })
}

fn tool_mix(rows: &[AgentRow]) -> BTreeMap<String, u64> {
    let mut out: BTreeMap<String, u64> = BTreeMap::new();
    for r in rows {
        if !r.tool.is_empty() {
            *out.entry(r.tool.clone()).or_default() += 1;
        }
    }
    out
}

/// 1 - cosine similarity. Empty vectors collapse to 0.0; identical
/// shapes collapse to 0.0. Disjoint shapes hit 1.0.
fn cosine_distance(a: &BTreeMap<String, u64>, b: &BTreeMap<String, u64>) -> f64 {
    let mut keys: BTreeSet<&String> = BTreeSet::new();
    for k in a.keys() {
        keys.insert(k);
    }
    for k in b.keys() {
        keys.insert(k);
    }
    let mut dot = 0.0_f64;
    let mut norm_a = 0.0_f64;
    let mut norm_b = 0.0_f64;
    for k in &keys {
        let av = *a.get(*k).unwrap_or(&0) as f64;
        let bv = *b.get(*k).unwrap_or(&0) as f64;
        dot += av * bv;
        norm_a += av * av;
        norm_b += bv * bv;
    }
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    let sim = dot / (norm_a.sqrt() * norm_b.sqrt());
    (1.0 - sim).max(0.0)
}

/// NewAgent: any agent name in window not seen in baseline.
fn new_agent(window: &[AgentRow], baseline: &[AgentRow]) -> Option<AnomalyDetection> {
    let baseline_agents: BTreeSet<&str> = baseline.iter().map(|r| r.agent.as_str()).collect();
    let mut window_counts: BTreeMap<String, u64> = BTreeMap::new();
    for r in window {
        *window_counts.entry(r.agent.clone()).or_default() += 1;
    }
    let mut new: Vec<(String, u64)> = window_counts
        .into_iter()
        .filter(|(name, _)| !baseline_agents.contains(name.as_str()))
        .collect();
    if new.is_empty() {
        return None;
    }
    new.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let top = new.first().cloned()?;
    let evidence: Vec<String> = new
        .iter()
        .take(EVIDENCE_LIMIT)
        .map(|(name, n)| format!("{name}: {n} call(s)"))
        .collect();
    Some(AnomalyDetection {
        kind: AnomalyKind::NewAgent,
        observed: top.1 as f64,
        baseline: 0.0,
        evidence,
    })
}

/// NewMcpMethod: any `events.method` in window not seen in baseline.
fn new_mcp_method(
    window: &HashMap<String, u64>,
    baseline: &HashMap<String, u64>,
) -> Option<AnomalyDetection> {
    let mut new: Vec<(String, u64)> = window
        .iter()
        .filter(|(name, _)| !baseline.contains_key(*name))
        .map(|(k, v)| (k.clone(), *v))
        .collect();
    if new.is_empty() {
        return None;
    }
    new.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let top = new.first().cloned()?;
    let evidence: Vec<String> = new
        .iter()
        .take(EVIDENCE_LIMIT)
        .map(|(name, n)| format!("{name}: {n} call(s)"))
        .collect();
    Some(AnomalyDetection {
        kind: AnomalyKind::NewMcpMethod,
        observed: top.1 as f64,
        baseline: 0.0,
        evidence,
    })
}

/// CostPerCallRise: window's avg-cost-per-call > 2× baseline's avg.
fn cost_per_call_rise(
    conn: &Connection,
    window_start: DateTime<Utc>,
    window_end: DateTime<Utc>,
    baseline_start: DateTime<Utc>,
    baseline_end: DateTime<Utc>,
    _baseline_days: f64,
    _window_days: f64,
) -> Result<Option<AnomalyDetection>, String> {
    let window = sum_cost(conn, window_start, window_end)?;
    let baseline = sum_cost(conn, baseline_start, baseline_end)?;
    if window.calls < FILE_EDIT_MIN || baseline.calls < FILE_EDIT_MIN {
        return Ok(None);
    }
    let window_avg = if window.calls == 0 {
        0.0
    } else {
        window.usd / window.calls as f64
    };
    let baseline_avg = if baseline.calls == 0 {
        0.0
    } else {
        baseline.usd / baseline.calls as f64
    };
    if baseline_avg <= 0.0 || window_avg < COST_RATIO * baseline_avg {
        return Ok(None);
    }
    let evidence = vec![
        format!("window: {} calls, ${:.4} (${:.5}/call)", window.calls, window.usd, window_avg),
        format!(
            "baseline: {} calls, ${:.4} (${:.5}/call)",
            baseline.calls, baseline.usd, baseline_avg
        ),
        format!("ratio {:.1}×", window_avg / baseline_avg),
    ];
    Ok(Some(AnomalyDetection {
        kind: AnomalyKind::CostPerCallRise,
        observed: window_avg,
        baseline: baseline_avg,
        evidence,
    }))
}

#[derive(Debug, Default)]
struct CostBucket {
    calls: u64,
    usd: f64,
}

/// Sum cost over a window using the same heuristic as
/// `crate::cost::collect_cost`: parsed MCP usage when available, falls
/// back to char-length heuristic on `agent_events`. Per-row mode keeps
/// us out of the cross-table query that `collect_cost` runs (we only
/// need the totals for the ratio test).
fn sum_cost(
    conn: &Connection,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Result<CostBucket, String> {
    let mut bucket = CostBucket::default();

    let start_str = start.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let end_str = end.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

    if let Ok(mut stmt) = conn.prepare(
        "SELECT agent, COALESCE(detail, '') AS detail \
         FROM agent_events WHERE ts >= ?1 AND ts < ?2",
    ) {
        let mapped = stmt
            .query_map([&start_str, &end_str], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })
            .map_err(|e| format!("query(anomaly cost agent_events): {e}"))?;
        for row in mapped {
            let (agent, detail) = row.map_err(|e| format!("row(anomaly cost): {e}"))?;
            bucket.calls += 1;
            if detail.is_empty() {
                continue;
            }
            let model = match default_model_for_agent(&agent) {
                Some(m) => m,
                None => continue,
            };
            let entry = match lookup(model) {
                Some(p) => p,
                None => continue,
            };
            let usage = heuristic_from_detail(&detail);
            bucket.usd += cost_for_usage(entry, &usage);
        }
    }

    let start_ms = start.timestamp_millis();
    let end_ms = end.timestamp_millis();
    if let Ok(mut stmt) = conn.prepare(
        "SELECT message_json FROM events WHERE ts_ms >= ?1 AND ts_ms < ?2",
    ) {
        let mapped = stmt
            .query_map([&start_ms, &end_ms], |r| r.get::<_, String>(0))
            .map_err(|e| format!("query(anomaly cost events): {e}"))?;
        for row in mapped.flatten() {
            let msg = match parse_message(&row) {
                Some(m) => m,
                None => continue,
            };
            // Only count the response side as a billable call so the
            // c2s + s2c pair doesn't double-count.
            let usage = match msg.usage {
                Some(u) => u,
                None => continue,
            };
            bucket.calls += 1;
            let model = msg.model.unwrap_or_else(|| "claude-haiku-4-5".to_string());
            let entry = match lookup(&model) {
                Some(p) => p,
                None => continue,
            };
            let parsed = ParsedUsage {
                input_tokens: usage.input_tokens,
                output_tokens: usage.output_tokens,
                cache_read_tokens: usage.cache_read_tokens,
                cache_write_tokens: usage.cache_write_tokens,
            };
            bucket.usd += cost_for_usage(entry, &parsed);
        }
    }

    Ok(bucket)
}

fn days_in_range(start: DateTime<Utc>, end: DateTime<Utc>) -> i64 {
    let secs = (end - start).num_seconds().max(0);
    (secs / 86_400).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

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

    fn open() -> (tempfile::TempDir, Connection) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("events.db");
        let conn = Connection::open(&path).unwrap();
        schema(&conn);
        (dir, conn)
    }

    fn anchor_now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 4, 26, 12, 0, 0).unwrap()
    }

    #[test]
    fn no_events_means_no_detections() {
        let (_d, conn) = open();
        let detections = detect_anomalies_at(&conn, &chrono_tz::UTC, AnomalyWindow::Today, anchor_now())
            .unwrap();
        assert!(detections.is_empty());
    }

    #[test]
    fn cosine_distance_zero_for_identical_mix() {
        let mut a = BTreeMap::new();
        a.insert("Edit".to_string(), 5_u64);
        a.insert("Bash".to_string(), 2_u64);
        let b = a.clone();
        assert!((cosine_distance(&a, &b) - 0.0).abs() < 1e-9);
    }

    #[test]
    fn cosine_distance_one_for_disjoint_mix() {
        let mut a = BTreeMap::new();
        a.insert("Edit".to_string(), 1_u64);
        let mut b = BTreeMap::new();
        b.insert("Bash".to_string(), 1_u64);
        assert!((cosine_distance(&a, &b) - 1.0).abs() < 1e-9);
    }
}
