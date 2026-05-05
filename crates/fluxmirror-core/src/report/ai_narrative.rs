// Phase 4 M-A2 — daily narrative heuristic fallback.
//
// The LLM-driven `synthesise_daily` path lives in the `fluxmirror-ai`
// crate (it has to: `fluxmirror-ai` already depends on this crate, so
// putting the LLM wrapper here would introduce a cycle). What lives in
// `fluxmirror-core` is the deterministic, network-free fallback used
// whenever the AI provider is `"off"`, the daily budget is exhausted,
// the cache + budget store can't be opened, or the provider returns an
// error. The fallback is also the unit of test coverage for the
// "narrative present + non-empty for any TodayData" property — keeping
// it here means the test can be a plain `cargo test -p fluxmirror-core`
// run with no AI dependency at all.
//
// `heuristic_paragraph` is the contract: 2–3 sentences, lower-case
// `(estimate)` is appended by the renderer not here, no panics on
// degenerate input. The only inputs are the already-aggregated
// counts on `TodayData`; nothing here re-queries the SQLite store.

use crate::report::dto::TodayData;

/// Build a deterministic 2–3 sentence summary of the day from the
/// already-aggregated `TodayData`. Used by `fluxmirror_ai::synthesise_daily`
/// as its always-on fallback path; also surfaced directly by the CLI
/// when the operator runs `fluxmirror today` with provider="off".
///
/// Properties (held by the unit tests):
///
///   * Always returns a non-empty string, even for an empty window.
///   * Never panics on degenerate inputs (zero events, no agents,
///     missing tool mix, single-agent with empty `top_tool`, …).
///   * Stable byte-for-byte for the same `TodayData`.
pub fn heuristic_paragraph(today: &TodayData) -> String {
    if today.total_events == 0 {
        return format!(
            "No agent activity recorded on {} ({}). The window is quiet — \
             nothing to summarise yet.",
            today.date, today.tz
        );
    }

    let lifecycle = lifecycle_label(today);
    let opener = opener_for(lifecycle, today);
    let middle = middle_for(today);
    let closer = closer_for(today);

    // Trim consecutive whitespace and ensure a single trailing newline-free
    // paragraph: the renderers that consume this string add their own
    // surrounding markup.
    let paragraph = format!("{} {} {}", opener, middle, closer);
    collapse_whitespace(&paragraph)
}

/// Coarse working pattern derived from the day's totals. Keeps the same
/// vocabulary as `lifecycle_hint` in `data.rs` so the prompt context and
/// the heuristic agree.
fn lifecycle_label(today: &TodayData) -> &'static str {
    if today.total_events == 0 {
        return "idle";
    }
    if today.total_events < 50 {
        return "light";
    }
    let writes = today.writes_total;
    let reads = today.reads_total;
    let shells = today.shells.len() as u64;
    if reads > writes.saturating_mul(2) && shells > 0 {
        return "investigating";
    }
    if writes > reads {
        return "building";
    }
    if reads > writes {
        return "polishing";
    }
    "balanced"
}

fn opener_for(lifecycle: &str, today: &TodayData) -> String {
    let calls = today.total_events;
    let agent_label = match today.agents.len() {
        0 => "an unspecified agent".to_string(),
        1 => today.agents[0].agent.clone(),
        2 => format!("{} and {}", today.agents[0].agent, today.agents[1].agent),
        n => format!(
            "{} and {} more agent{}",
            today.agents[0].agent,
            n - 1,
            if n - 1 == 1 { "" } else { "s" }
        ),
    };
    let shape = match lifecycle {
        "light" => "a light check-in",
        "building" => "a build-heavy session",
        "polishing" => "a read-and-polish pass",
        "investigating" => "an investigation",
        "balanced" => "a mixed session",
        _ => "a session",
    };
    format!(
        "{shape} on {date} ({tz}) — {calls} call{plural} through {agent_label}.",
        shape = capitalise_first(shape),
        date = today.date,
        tz = today.tz,
        calls = calls,
        plural = if calls == 1 { "" } else { "s" },
        agent_label = agent_label,
    )
}

fn middle_for(today: &TodayData) -> String {
    let writes = today.writes_total;
    let reads = today.reads_total;
    let edited = today.files_edited.len();
    let read_only = today.files_read.len();

    if writes == 0 && reads == 0 {
        return "No file edits or reads landed in the window.".to_string();
    }
    if writes == 0 {
        return format!(
            "No write-class events; {reads} read{rp} across {read_only} file{fp}.",
            reads = reads,
            rp = if reads == 1 { "" } else { "s" },
            read_only = read_only,
            fp = if read_only == 1 { "" } else { "s" },
        );
    }
    if reads == 0 {
        return format!(
            "{writes} write{wp} across {edited} file{fp}, no read-class events.",
            writes = writes,
            wp = if writes == 1 { "" } else { "s" },
            edited = edited,
            fp = if edited == 1 { "" } else { "s" },
        );
    }
    let ratio = writes as f64 / reads as f64;
    format!(
        "{writes} write{wp} on {edited} file{efp} against {reads} read{rp} on {read_only} file{rfp} (edit/read {ratio:.2}).",
        writes = writes,
        wp = if writes == 1 { "" } else { "s" },
        edited = edited,
        efp = if edited == 1 { "" } else { "s" },
        reads = reads,
        rp = if reads == 1 { "" } else { "s" },
        read_only = read_only,
        rfp = if read_only == 1 { "" } else { "s" },
        ratio = ratio,
    )
}

fn closer_for(today: &TodayData) -> String {
    let top_tool = today
        .tool_mix
        .first()
        .map(|t| (t.tool.clone(), t.count))
        .filter(|(name, _)| !name.is_empty());
    let top_file = today
        .files_edited
        .first()
        .map(|f| f.path.clone())
        .filter(|p| !p.is_empty());

    match (top_tool, top_file) {
        (Some((tool, count)), Some(file)) => format!(
            "Dominant tool was {tool} ({count} call{plural}); the spine file was {file}.",
            tool = tool,
            count = count,
            plural = if count == 1 { "" } else { "s" },
            file = file,
        ),
        (Some((tool, count)), None) => format!(
            "Dominant tool was {tool} ({count} call{plural}); no file edits were attributed.",
            tool = tool,
            count = count,
            plural = if count == 1 { "" } else { "s" },
        ),
        (None, Some(file)) => format!(
            "Edits centred on {file}.",
            file = file,
        ),
        (None, None) => "No dominant tool or file emerged from the window.".to_string(),
    }
}

fn capitalise_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn collapse_whitespace(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_space = false;
    for c in s.chars() {
        if c.is_whitespace() {
            if !prev_space && !out.is_empty() {
                out.push(' ');
            }
            prev_space = true;
        } else {
            out.push(c);
            prev_space = false;
        }
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::dto::{
        AgentCount, FileTouch, ShellEvent, ToolMixEntry,
    };
    use chrono::{NaiveDate, Utc};

    fn empty_today() -> TodayData {
        TodayData {
            date: NaiveDate::from_ymd_opt(2026, 4, 26).unwrap(),
            tz: "UTC".to_string(),
            ..Default::default()
        }
    }

    fn busy_today() -> TodayData {
        let mut t = empty_today();
        t.total_events = 120;
        t.writes_total = 40;
        t.reads_total = 60;
        t.agents = vec![
            AgentCount {
                agent: "claude-code".into(),
                calls: 90,
                sessions: vec!["s1".into()],
                active_days: vec![],
                top_tool: "Edit".into(),
            },
            AgentCount {
                agent: "gemini-cli".into(),
                calls: 30,
                sessions: vec!["g1".into()],
                active_days: vec![],
                top_tool: "edit_file".into(),
            },
        ];
        t.files_edited = vec![FileTouch {
            path: "src/lib.rs".into(),
            tool: "Edit".into(),
            count: 14,
        }];
        t.tool_mix = vec![
            ToolMixEntry {
                tool: "Edit".into(),
                count: 35,
            },
            ToolMixEntry {
                tool: "Read".into(),
                count: 25,
            },
        ];
        t.shells = vec![ShellEvent {
            time_local: "10:00".into(),
            detail: "cargo test".into(),
            ts_utc: Utc::now(),
        }];
        t
    }

    #[test]
    fn empty_window_returns_non_empty_paragraph() {
        let p = heuristic_paragraph(&empty_today());
        assert!(!p.is_empty());
        assert!(p.contains("2026-04-26"));
        assert!(p.to_lowercase().contains("no agent activity"));
    }

    #[test]
    fn busy_window_mentions_top_tool_and_top_file() {
        let p = heuristic_paragraph(&busy_today());
        assert!(p.contains("Edit"));
        assert!(p.contains("src/lib.rs"));
        assert!(p.contains("claude-code"));
    }

    #[test]
    fn deterministic_paragraph_for_same_input() {
        let t = busy_today();
        let a = heuristic_paragraph(&t);
        let b = heuristic_paragraph(&t);
        assert_eq!(a, b);
    }

    #[test]
    fn degenerate_zero_reads_writes_does_not_panic() {
        let mut t = empty_today();
        t.total_events = 5;
        let _ = heuristic_paragraph(&t);
    }

    #[test]
    fn missing_tool_mix_falls_back_gracefully() {
        let mut t = busy_today();
        t.tool_mix.clear();
        t.files_edited.clear();
        let p = heuristic_paragraph(&t);
        assert!(p.contains("No dominant tool"));
    }

    #[test]
    fn lifecycle_label_classifies_basic_shapes() {
        let mut t = empty_today();
        assert_eq!(lifecycle_label(&t), "idle");
        t.total_events = 10;
        assert_eq!(lifecycle_label(&t), "light");
        t.total_events = 100;
        t.writes_total = 60;
        t.reads_total = 30;
        assert_eq!(lifecycle_label(&t), "building");
        t.writes_total = 20;
        t.reads_total = 70;
        assert_eq!(lifecycle_label(&t), "polishing");
    }
}
