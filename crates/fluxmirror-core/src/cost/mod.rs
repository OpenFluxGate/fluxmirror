// Cost overlay — token usage extraction + per-model pricing lookup.
//
// Phase 3 M6 wires an estimated USD figure into every report surface.
// The signal is best-effort by design:
//
//   - For MCP traffic (the `events` table written by `fluxmirror proxy`)
//     we parse the Anthropic-shaped `usage` block out of every result
//     line and pair it with the model id from `result.model` (or the
//     matching request line's `params.model` when the response doesn't
//     echo it). These tokens are real.
//
//   - For non-MCP agent activity (every row in `agent_events`) we have
//     no token counts at all. We synthesise a heuristic from the
//     detail-string length and flag the result as an estimate so the UI
//     can render it differently. The heuristic uses `len(detail) * 1.3 / 4`
//     as the input token count and `2 * input` for the output count —
//     deliberately rough.
//
// Pricing lives in a static table; sources are documented separately
// in `docs/PRICING_SOURCES.md`. Quarterly manual refresh is acknowledged
// — this is a single-user project, not a billing system.

use std::collections::BTreeMap;

use chrono_tz::Tz;
use rusqlite::Connection;
use serde_json::Value;

use crate::report::dto::{AgentCost, CostSummary, ModelCost, WindowRange};

/// One row of the static pricing table. All `*_per_mtok_usd` fields
/// are USD per **million** tokens, the unit every public price page
/// uses today. Cache rates are optional because not every model exposes
/// caching (or pricing for it).
#[derive(Debug, Clone, Copy)]
pub struct PricingEntry {
    pub provider: &'static str,
    pub model: &'static str,
    pub input_per_mtok_usd: f64,
    pub output_per_mtok_usd: f64,
    pub cache_read_per_mtok_usd: Option<f64>,
    pub cache_write_per_mtok_usd: Option<f64>,
}

/// Static price table. Entries are looked up by model id (exact or
/// prefix match — see [`lookup`]). Source URLs for each entry live in
/// `docs/PRICING_SOURCES.md`; comments here cite the same URLs so a
/// reader doesn't have to context-switch.
pub const PRICING: &[PricingEntry] = &[
    // anthropic — https://www.anthropic.com/pricing#api
    PricingEntry {
        provider: "anthropic",
        model: "claude-opus-4-7",
        input_per_mtok_usd: 15.0,
        output_per_mtok_usd: 75.0,
        cache_read_per_mtok_usd: Some(1.50),
        cache_write_per_mtok_usd: Some(18.75),
    },
    PricingEntry {
        provider: "anthropic",
        model: "claude-opus-4",
        input_per_mtok_usd: 15.0,
        output_per_mtok_usd: 75.0,
        cache_read_per_mtok_usd: Some(1.50),
        cache_write_per_mtok_usd: Some(18.75),
    },
    PricingEntry {
        provider: "anthropic",
        model: "claude-sonnet-4-6",
        input_per_mtok_usd: 3.0,
        output_per_mtok_usd: 15.0,
        cache_read_per_mtok_usd: Some(0.30),
        cache_write_per_mtok_usd: Some(3.75),
    },
    PricingEntry {
        provider: "anthropic",
        model: "claude-sonnet-4",
        input_per_mtok_usd: 3.0,
        output_per_mtok_usd: 15.0,
        cache_read_per_mtok_usd: Some(0.30),
        cache_write_per_mtok_usd: Some(3.75),
    },
    PricingEntry {
        provider: "anthropic",
        model: "claude-haiku-4-5",
        input_per_mtok_usd: 1.0,
        output_per_mtok_usd: 5.0,
        cache_read_per_mtok_usd: Some(0.10),
        cache_write_per_mtok_usd: Some(1.25),
    },
    PricingEntry {
        provider: "anthropic",
        model: "claude-3-5-sonnet",
        input_per_mtok_usd: 3.0,
        output_per_mtok_usd: 15.0,
        cache_read_per_mtok_usd: Some(0.30),
        cache_write_per_mtok_usd: Some(3.75),
    },
    PricingEntry {
        provider: "anthropic",
        model: "claude-3-5-haiku",
        input_per_mtok_usd: 0.80,
        output_per_mtok_usd: 4.0,
        cache_read_per_mtok_usd: Some(0.08),
        cache_write_per_mtok_usd: Some(1.0),
    },
    PricingEntry {
        provider: "anthropic",
        model: "claude-3-opus",
        input_per_mtok_usd: 15.0,
        output_per_mtok_usd: 75.0,
        cache_read_per_mtok_usd: Some(1.50),
        cache_write_per_mtok_usd: Some(18.75),
    },
    // openai — https://openai.com/api/pricing/
    PricingEntry {
        provider: "openai",
        model: "gpt-4-turbo",
        input_per_mtok_usd: 10.0,
        output_per_mtok_usd: 30.0,
        cache_read_per_mtok_usd: None,
        cache_write_per_mtok_usd: None,
    },
    PricingEntry {
        provider: "openai",
        model: "gpt-4o",
        input_per_mtok_usd: 2.50,
        output_per_mtok_usd: 10.0,
        cache_read_per_mtok_usd: Some(1.25),
        cache_write_per_mtok_usd: None,
    },
    PricingEntry {
        provider: "openai",
        model: "gpt-4o-mini",
        input_per_mtok_usd: 0.15,
        output_per_mtok_usd: 0.60,
        cache_read_per_mtok_usd: Some(0.075),
        cache_write_per_mtok_usd: None,
    },
    // google — https://ai.google.dev/pricing
    PricingEntry {
        provider: "google",
        model: "gemini-2.5-pro",
        input_per_mtok_usd: 1.25,
        output_per_mtok_usd: 10.0,
        cache_read_per_mtok_usd: Some(0.31),
        cache_write_per_mtok_usd: None,
    },
    PricingEntry {
        provider: "google",
        model: "gemini-2.5-flash",
        input_per_mtok_usd: 0.30,
        output_per_mtok_usd: 2.50,
        cache_read_per_mtok_usd: Some(0.075),
        cache_write_per_mtok_usd: None,
    },
    PricingEntry {
        provider: "google",
        model: "gemini-1.5-pro",
        input_per_mtok_usd: 1.25,
        output_per_mtok_usd: 5.0,
        cache_read_per_mtok_usd: Some(0.3125),
        cache_write_per_mtok_usd: None,
    },
    PricingEntry {
        provider: "google",
        model: "gemini-1.5-flash",
        input_per_mtok_usd: 0.075,
        output_per_mtok_usd: 0.30,
        cache_read_per_mtok_usd: Some(0.01875),
        cache_write_per_mtok_usd: None,
    },
];

/// Resolve a model id (e.g. `claude-opus-4-7`, `claude-3-5-sonnet-20241022`,
/// `gemini-2.5-pro`) to a pricing entry. Tries an exact match first,
/// then falls back to a longest-prefix match so dated suffixes like
/// `-20241022` still resolve. Unknown models return `None`; callers
/// should treat that as zero-cost.
pub fn lookup(model: &str) -> Option<&'static PricingEntry> {
    let key = model.trim().to_ascii_lowercase();
    if key.is_empty() {
        return None;
    }
    for entry in PRICING {
        if entry.model.eq_ignore_ascii_case(&key) {
            return Some(entry);
        }
    }
    // Longest-prefix wins so `claude-3-5-sonnet` beats `claude-3-5`.
    let mut by_len: Vec<&PricingEntry> = PRICING.iter().collect();
    by_len.sort_by_key(|e| std::cmp::Reverse(e.model.len()));
    for entry in by_len {
        if key.starts_with(entry.model) {
            return Some(entry);
        }
    }
    None
}

/// Default model id used when computing a heuristic estimate for an
/// agent that doesn't carry MCP usage data. None when no defensible
/// default exists — the caller still tracks tokens but charges 0 USD.
pub fn default_model_for_agent(agent: &str) -> Option<&'static str> {
    match agent {
        "claude-code" => Some("claude-sonnet-4-6"),
        "claude-desktop" => Some("claude-sonnet-4-6"),
        "gemini-cli" => Some("gemini-2.5-pro"),
        // qwen-code has no obvious default — leave the cost at zero
        // rather than guess a third-party rate.
        _ => None,
    }
}

/// Anthropic-shaped `usage` block extracted from one MCP response line.
/// All counts are in tokens. Missing cache fields default to zero so
/// downstream cost math doesn't have to special-case them.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct ParsedUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
}

impl ParsedUsage {
    /// Sum of every token category. Used by callers that don't care
    /// about the input/output split (e.g. estimate_share weighting).
    pub fn total(&self) -> u64 {
        self.input_tokens
            .saturating_add(self.output_tokens)
            .saturating_add(self.cache_read_tokens)
            .saturating_add(self.cache_write_tokens)
    }
}

/// One MCP message extraction. `model` and `usage` are independent
/// signals — a request line carries the model but no usage; a response
/// line typically carries usage and may or may not echo the model. The
/// caller pairs the two by walking the events sorted by timestamp.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct MessageExtract {
    pub model: Option<String>,
    pub usage: Option<ParsedUsage>,
}

/// Parse one NDJSON line from `events.message_json`. Returns `None`
/// when the line is malformed; callers skip those lines silently. A
/// well-formed line with no usage block returns `Some` with `usage =
/// None` so the caller can still capture the model id.
pub fn parse_message(line: &str) -> Option<MessageExtract> {
    let v: Value = serde_json::from_str(line).ok()?;
    let mut out = MessageExtract::default();

    // Model lookup: result.model first (response), then params.model
    // (request). Some servers also nest params.arguments.model — try
    // that as a last resort.
    if let Some(m) = pluck_str(&v, &["result", "model"]) {
        out.model = Some(m);
    } else if let Some(m) = pluck_str(&v, &["params", "model"]) {
        out.model = Some(m);
    } else if let Some(m) = pluck_str(&v, &["params", "arguments", "model"]) {
        out.model = Some(m);
    }

    // Usage lookup: result.usage is the canonical Anthropic shape.
    // Some servers nest it as result.message.usage (Claude Code's
    // intermediate format); try both.
    if let Some(u) = v.pointer("/result/usage").and_then(parse_usage_obj) {
        out.usage = Some(u);
    } else if let Some(u) = v.pointer("/result/message/usage").and_then(parse_usage_obj) {
        out.usage = Some(u);
    }

    Some(out)
}

fn pluck_str(v: &Value, path: &[&str]) -> Option<String> {
    let mut cur = v;
    for k in path {
        cur = cur.get(*k)?;
    }
    cur.as_str().map(|s| s.to_string())
}

fn parse_usage_obj(v: &Value) -> Option<ParsedUsage> {
    let obj = v.as_object()?;
    let read_u64 = |k: &str| -> u64 {
        obj.get(k)
            .and_then(|x| x.as_u64())
            .unwrap_or(0)
    };
    let usage = ParsedUsage {
        input_tokens: read_u64("input_tokens"),
        output_tokens: read_u64("output_tokens"),
        cache_read_tokens: read_u64("cache_read_input_tokens"),
        cache_write_tokens: read_u64("cache_creation_input_tokens"),
    };
    if usage.total() == 0 {
        // A usage block with every field zero / missing is noise — don't
        // surface it as a real signal.
        return None;
    }
    Some(usage)
}

/// Heuristic token estimate from an agent_events row's `detail` field.
/// Deliberately rough — calibrated to "in the same order of magnitude
/// as reality" rather than precise. Documented in CLAUDE.md as
/// estimate-only signal.
pub fn heuristic_from_detail(detail: &str) -> ParsedUsage {
    if detail.is_empty() {
        return ParsedUsage::default();
    }
    let chars = detail.chars().count() as f64;
    let input = (chars * 1.3 / 4.0).max(1.0).round() as u64;
    let output = input.saturating_mul(2);
    ParsedUsage {
        input_tokens: input,
        output_tokens: output,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
    }
}

/// USD cost of one token bucket given the model's pricing entry. None
/// when the entry doesn't list a cache rate — callers fall back to the
/// input rate for cache reads and a 1.25x input rate for cache writes
/// to match Anthropic's published pattern.
pub fn cost_for_usage(entry: &PricingEntry, usage: &ParsedUsage) -> f64 {
    let m = 1_000_000.0;
    let input = usage.input_tokens as f64 / m * entry.input_per_mtok_usd;
    let output = usage.output_tokens as f64 / m * entry.output_per_mtok_usd;
    let cache_read = usage.cache_read_tokens as f64 / m
        * entry
            .cache_read_per_mtok_usd
            .unwrap_or(entry.input_per_mtok_usd);
    let cache_write = usage.cache_write_tokens as f64 / m
        * entry
            .cache_write_per_mtok_usd
            .unwrap_or(entry.input_per_mtok_usd * 1.25);
    input + output + cache_read + cache_write
}

/// Per-(agent, model) intermediate aggregate before we fold into the
/// `CostSummary` DTO. Real (MCP-parsed) and heuristic rows are tracked
/// separately so the top-level `estimate_share` is meaningful.
#[derive(Debug, Default, Clone)]
struct CostBucket {
    real_in: u64,
    real_out: u64,
    real_cache_read: u64,
    real_cache_write: u64,
    real_usd: f64,
    est_in: u64,
    est_out: u64,
    est_usd: f64,
}

impl CostBucket {
    fn add_real(&mut self, usage: &ParsedUsage, usd: f64) {
        self.real_in = self.real_in.saturating_add(usage.input_tokens);
        self.real_out = self.real_out.saturating_add(usage.output_tokens);
        self.real_cache_read = self
            .real_cache_read
            .saturating_add(usage.cache_read_tokens);
        self.real_cache_write = self
            .real_cache_write
            .saturating_add(usage.cache_write_tokens);
        self.real_usd += usd;
    }
    fn add_est(&mut self, usage: &ParsedUsage, usd: f64) {
        self.est_in = self.est_in.saturating_add(usage.input_tokens);
        self.est_out = self.est_out.saturating_add(usage.output_tokens);
        self.est_usd += usd;
    }
    fn total_in(&self) -> u64 {
        self.real_in.saturating_add(self.est_in)
    }
    fn total_out(&self) -> u64 {
        self.real_out.saturating_add(self.est_out)
    }
    fn total_usd(&self) -> f64 {
        self.real_usd + self.est_usd
    }
    fn estimate_only(&self) -> bool {
        self.real_in == 0
            && self.real_out == 0
            && self.real_cache_read == 0
            && self.real_cache_write == 0
            && (self.est_in > 0 || self.est_out > 0)
    }
}

/// Aggregate cost over a UTC window. Reads both the `events` table
/// (MCP-parsed, real tokens) and the `agent_events` table (heuristic,
/// estimated tokens) and folds them into a single [`CostSummary`].
pub fn collect_cost(
    conn: &Connection,
    _tz: &Tz,
    range: WindowRange,
) -> Result<CostSummary, String> {
    let mut by_agent: BTreeMap<String, CostBucket> = BTreeMap::new();
    let mut by_model: BTreeMap<String, CostBucket> = BTreeMap::new();

    // ---- MCP events: real tokens, real model ids -----------------------
    collect_mcp_into(conn, &range, &mut by_agent, &mut by_model)?;

    // ---- agent_events: heuristic tokens, default model per agent --------
    collect_heuristic_into(conn, &range, &mut by_agent, &mut by_model)?;

    let mut total_real_usd = 0.0f64;
    let mut total_est_usd = 0.0f64;
    for b in by_agent.values() {
        total_real_usd += b.real_usd;
        total_est_usd += b.est_usd;
    }
    let total_usd = total_real_usd + total_est_usd;
    let estimate_share = if total_usd > 0.0 {
        (total_est_usd / total_usd).clamp(0.0, 1.0)
    } else {
        // No dollars in either bucket. Fall back to the token-share so
        // a heuristic-only window still reports `estimate_share = 1.0`
        // instead of a misleading 0.0.
        let mut real_tok = 0u64;
        let mut est_tok = 0u64;
        for b in by_agent.values() {
            real_tok = real_tok
                .saturating_add(b.real_in)
                .saturating_add(b.real_out)
                .saturating_add(b.real_cache_read)
                .saturating_add(b.real_cache_write);
            est_tok = est_tok.saturating_add(b.est_in).saturating_add(b.est_out);
        }
        let denom = real_tok.saturating_add(est_tok);
        if denom == 0 {
            0.0
        } else {
            (est_tok as f64 / denom as f64).clamp(0.0, 1.0)
        }
    };

    let mut by_agent_out: Vec<AgentCost> = by_agent
        .into_iter()
        .map(|(agent, b)| AgentCost {
            usd: round_cents(b.total_usd()),
            tokens_in: b.total_in(),
            tokens_out: b.total_out(),
            estimate: b.estimate_only(),
            agent,
        })
        .collect();
    by_agent_out.sort_by(|a, b| {
        b.usd
            .partial_cmp(&a.usd)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.agent.cmp(&b.agent))
    });

    let mut by_model_out: Vec<ModelCost> = by_model
        .into_iter()
        .map(|(model, b)| ModelCost {
            usd: round_cents(b.total_usd()),
            tokens_in: b.total_in(),
            tokens_out: b.total_out(),
            estimate: b.estimate_only(),
            model,
        })
        .collect();
    by_model_out.sort_by(|a, b| {
        b.usd
            .partial_cmp(&a.usd)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.model.cmp(&b.model))
    });

    Ok(CostSummary {
        from: range
            .start_utc
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        to: range
            .end_utc
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        total_usd: round_cents(total_usd),
        by_agent: by_agent_out,
        by_model: by_model_out,
        estimate_share: (estimate_share * 1_000.0).round() / 1_000.0,
    })
}

fn round_cents(usd: f64) -> f64 {
    (usd * 10_000.0).round() / 10_000.0
}

fn collect_mcp_into(
    conn: &Connection,
    range: &WindowRange,
    by_agent: &mut BTreeMap<String, CostBucket>,
    by_model: &mut BTreeMap<String, CostBucket>,
) -> Result<(), String> {
    let start_ms = range.start_utc.timestamp_millis();
    let end_ms = range.end_utc.timestamp_millis();
    let mut stmt = match conn.prepare(
        "SELECT direction, COALESCE(method, ''), message_json \
         FROM events WHERE ts_ms >= ?1 AND ts_ms < ?2 ORDER BY ts_ms ASC, id ASC",
    ) {
        Ok(s) => s,
        // Legacy DBs without the proxy migration won't have the table;
        // treat as empty rather than failing the whole report.
        Err(_) => return Ok(()),
    };
    let rows = match stmt.query_map([start_ms, end_ms], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
        ))
    }) {
        Ok(it) => it,
        Err(_) => return Ok(()),
    };

    // Sticky-model heuristic: walk lines in order. A request line that
    // carries `params.model` (or similar) sets the "current" model; a
    // following response with usage but no echoed model inherits it.
    // Resets on each new request line.
    let mut sticky_model: Option<String> = None;
    for row in rows.flatten() {
        let (direction, _method, line) = row;
        let extract = match parse_message(&line) {
            Some(e) => e,
            None => continue,
        };

        if direction == "c2s" {
            if let Some(m) = extract.model.clone() {
                sticky_model = Some(m);
            }
            continue;
        }

        // s2c branch — usage, if any, is the real cost signal.
        let usage = match extract.usage {
            Some(u) => u,
            None => continue,
        };
        let model = extract.model.or_else(|| sticky_model.clone());
        let model_str = model.unwrap_or_else(|| "unknown".to_string());

        let usd = match lookup(&model_str) {
            Some(p) => cost_for_usage(p, &usage),
            None => 0.0,
        };

        // MCP traffic is attributed to the proxy's source agent. The
        // events table doesn't carry the agent label directly, so we
        // bucket under "claude-desktop" — the only agent that runs
        // through `fluxmirror proxy` today.
        let agent_key = "claude-desktop".to_string();
        by_agent
            .entry(agent_key)
            .or_default()
            .add_real(&usage, usd);
        by_model
            .entry(model_str)
            .or_default()
            .add_real(&usage, usd);
    }
    Ok(())
}

fn collect_heuristic_into(
    conn: &Connection,
    range: &WindowRange,
    by_agent: &mut BTreeMap<String, CostBucket>,
    by_model: &mut BTreeMap<String, CostBucket>,
) -> Result<(), String> {
    let start_str = range
        .start_utc
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let end_str = range
        .end_utc
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

    let mut stmt = conn
        .prepare(
            "SELECT agent, COALESCE(detail, '') AS detail \
             FROM agent_events WHERE ts >= ?1 AND ts < ?2",
        )
        .map_err(|e| format!("prepare(cost agent_events): {e}"))?;
    let rows = stmt
        .query_map([&start_str, &end_str], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })
        .map_err(|e| format!("query(cost agent_events): {e}"))?;

    for row in rows.flatten() {
        let (agent, detail) = row;
        let usage = heuristic_from_detail(&detail);
        if usage.total() == 0 {
            continue;
        }
        let model = default_model_for_agent(&agent).unwrap_or("unknown");
        let usd = match lookup(model) {
            Some(p) => cost_for_usage(p, &usage),
            None => 0.0,
        };
        by_agent
            .entry(agent.clone())
            .or_default()
            .add_est(&usage, usd);
        by_model
            .entry(model.to_string())
            .or_default()
            .add_est(&usage, usd);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, NaiveDate, Utc};

    #[test]
    fn lookup_exact_match_returns_entry() {
        let p = lookup("claude-opus-4-7").expect("must resolve");
        assert_eq!(p.provider, "anthropic");
        assert!((p.input_per_mtok_usd - 15.0).abs() < f64::EPSILON);
    }

    #[test]
    fn lookup_prefix_match_handles_dated_suffix() {
        let p = lookup("claude-3-5-sonnet-20241022").expect("must resolve via prefix");
        assert_eq!(p.model, "claude-3-5-sonnet");
    }

    #[test]
    fn lookup_unknown_returns_none() {
        assert!(lookup("not-a-real-model").is_none());
        assert!(lookup("").is_none());
    }

    #[test]
    fn lookup_case_insensitive() {
        assert!(lookup("Claude-Opus-4-7").is_some());
        assert!(lookup("GEMINI-2.5-PRO").is_some());
    }

    #[test]
    fn parse_message_returns_usage_block() {
        let line = r#"{"jsonrpc":"2.0","id":1,"result":{"model":"claude-opus-4-7","usage":{"input_tokens":1234,"output_tokens":567,"cache_creation_input_tokens":100,"cache_read_input_tokens":50}}}"#;
        let m = parse_message(line).expect("well-formed");
        assert_eq!(m.model.as_deref(), Some("claude-opus-4-7"));
        let u = m.usage.expect("usage block present");
        assert_eq!(u.input_tokens, 1234);
        assert_eq!(u.output_tokens, 567);
        assert_eq!(u.cache_write_tokens, 100);
        assert_eq!(u.cache_read_tokens, 50);
    }

    #[test]
    fn parse_message_skips_empty_usage() {
        let line = r#"{"jsonrpc":"2.0","id":1,"result":{"model":"claude-opus-4-7","usage":{"input_tokens":0,"output_tokens":0}}}"#;
        let m = parse_message(line).expect("well-formed");
        assert!(m.usage.is_none(), "all-zero usage must be filtered");
    }

    #[test]
    fn parse_message_picks_up_request_model() {
        let line = r#"{"jsonrpc":"2.0","id":1,"method":"messages/create","params":{"model":"gemini-2.5-pro","messages":[]}}"#;
        let m = parse_message(line).expect("well-formed");
        assert_eq!(m.model.as_deref(), Some("gemini-2.5-pro"));
        assert!(m.usage.is_none());
    }

    #[test]
    fn parse_message_returns_none_for_malformed() {
        assert!(parse_message("not a json").is_none());
        assert!(parse_message("{bad json").is_none());
    }

    #[test]
    fn heuristic_scales_with_detail_length() {
        let short = heuristic_from_detail("ls");
        let long = heuristic_from_detail(&"x".repeat(400));
        assert!(short.total() < long.total());
        assert_eq!(short.output_tokens, short.input_tokens * 2);
    }

    #[test]
    fn heuristic_empty_detail_is_zero() {
        let u = heuristic_from_detail("");
        assert_eq!(u.total(), 0);
    }

    #[test]
    fn heuristic_within_calibration_bounds() {
        // 400 chars of typical text → ~130 input tokens (rough). The
        // exact formula is `len * 1.3 / 4`, so 400 → 130. Output is
        // 2x input → 260. Sanity-check the bounds.
        let u = heuristic_from_detail(&"x".repeat(400));
        assert!(u.input_tokens >= 100 && u.input_tokens <= 200, "{u:?}");
        assert_eq!(u.output_tokens, u.input_tokens * 2);
    }

    #[test]
    fn cost_for_usage_basic_anthropic_math() {
        // 1M input @ $15 + 1M output @ $75 = $90.
        let entry = lookup("claude-opus-4-7").unwrap();
        let usage = ParsedUsage {
            input_tokens: 1_000_000,
            output_tokens: 1_000_000,
            ..Default::default()
        };
        let usd = cost_for_usage(entry, &usage);
        assert!((usd - 90.0).abs() < 1e-9, "got {usd}");
    }

    #[test]
    fn collect_cost_empty_window_is_zero() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("events.db");
        let conn = Connection::open(&path).unwrap();
        crate::cost::tests::install_schema(&conn);
        let tz: Tz = "UTC".parse().unwrap();
        let range = WindowRange {
            start_utc: "2026-04-26T00:00:00Z"
                .parse::<DateTime<Utc>>()
                .unwrap(),
            end_utc: "2026-04-27T00:00:00Z"
                .parse::<DateTime<Utc>>()
                .unwrap(),
            anchor_date: NaiveDate::from_ymd_opt(2026, 4, 26).unwrap(),
            tz: "UTC".to_string(),
        };
        let summary = collect_cost(&conn, &tz, range).unwrap();
        assert_eq!(summary.total_usd, 0.0);
        assert_eq!(summary.estimate_share, 0.0);
        assert!(summary.by_agent.is_empty());
        assert!(summary.by_model.is_empty());
    }

    pub(crate) fn install_schema(conn: &Connection) {
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
}
