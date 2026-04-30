// Shared `## Cost` section renderer for the day and week text reports.
//
// The cost overlay (Phase 3 M6) ships an estimated USD figure alongside
// every report. It is intentionally best-effort: MCP traffic in the
// `events` table contributes real tokens parsed from the Anthropic-shaped
// `usage` block; non-MCP `agent_events` rows contribute heuristic tokens
// flagged with `*` so a reader can tell them apart.
//
// The section is omitted entirely when the window has no cost signal at
// all (no real tokens, no heuristic tokens). Tiny non-zero windows still
// render — surfacing a `$0.00` line is informative.
//
// Format (markdown):
//
// ## Cost (estimate)
//
// Total: $1.23
//
// | Agent | Tokens in | Tokens out | USD |
// |---|---|---|---|
// | claude-desktop | 12,300 | 4,500 | 0.97 |
// | gemini-cli | 8,400 | 16,800 | 0.26* |
//
// 21% of this figure is estimated from non-MCP agent activity.

use fluxmirror_core::report::{CostSummary, LangPack};

/// Render the `## Cost` section into `out`. Caller decides whether to
/// emit it (typically only when `Some` is present).
pub(super) fn render_cost(out: &mut String, lp: &LangPack, summary: &CostSummary) {
    if summary.by_agent.is_empty() && summary.total_usd == 0.0 {
        return;
    }
    out.push_str(&format!("## {}\n\n", lp.cost_heading));
    out.push_str(&format!(
        "{}: ${}\n\n",
        lp.cost_total_label,
        format_usd(summary.total_usd)
    ));

    if !summary.by_agent.is_empty() {
        let cols = lp.cost_columns;
        out.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            cols[0], cols[1], cols[2], cols[3]
        ));
        out.push_str("|---|---|---|---|\n");
        for row in &summary.by_agent {
            let marker = if row.estimate {
                lp.cost_estimate_marker
            } else {
                ""
            };
            out.push_str(&format!(
                "| {} | {} | {} | {}{} |\n",
                row.agent,
                format_int(row.tokens_in),
                format_int(row.tokens_out),
                format_usd(row.usd),
                marker
            ));
        }
        out.push('\n');
    }

    if summary.estimate_share > 0.0 {
        let pct = (summary.estimate_share * 100.0).round() as u64;
        out.push_str(&format!(
            "{}\n\n",
            lp.cost_estimate_note.replace("{pct}", &pct.to_string())
        ));
    }
}

/// Format a USD figure with two decimals when the value is below 100,
/// four decimals when below 1 cent (so micropayments still surface),
/// and round to whole dollars otherwise.
pub(super) fn format_usd(usd: f64) -> String {
    if usd.abs() < 0.01 {
        format!("{:.4}", usd)
    } else if usd.abs() < 100.0 {
        format!("{:.2}", usd)
    } else {
        format!("{:.0}", usd.round())
    }
}

/// Format a token count with thousands separators using Rust's
/// no-allocation u64 formatter — kept as a tiny inline helper so both
/// the text and HTML cost renderers share the look.
pub(super) fn format_int(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out.chars().rev().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use fluxmirror_core::report::{pack, AgentCost, CostSummary, ModelCost};

    fn fixture(estimate_share: f64) -> CostSummary {
        CostSummary {
            from: "2026-04-26T00:00:00Z".to_string(),
            to: "2026-04-27T00:00:00Z".to_string(),
            total_usd: 1.23,
            by_agent: vec![
                AgentCost {
                    agent: "claude-desktop".into(),
                    usd: 0.97,
                    tokens_in: 12_300,
                    tokens_out: 4_500,
                    estimate: false,
                },
                AgentCost {
                    agent: "gemini-cli".into(),
                    usd: 0.26,
                    tokens_in: 8_400,
                    tokens_out: 16_800,
                    estimate: true,
                },
            ],
            by_model: vec![ModelCost {
                model: "claude-opus-4-7".into(),
                usd: 0.97,
                tokens_in: 12_300,
                tokens_out: 4_500,
                estimate: false,
            }],
            estimate_share,
        }
    }

    #[test]
    fn renders_full_cost_section() {
        let lp = pack("english");
        let mut out = String::new();
        render_cost(&mut out, lp, &fixture(0.21));
        assert!(out.contains("## Cost (estimate)"));
        assert!(out.contains("Total: $1.23"));
        assert!(out.contains("| claude-desktop | 12,300 | 4,500 | 0.97 |"));
        assert!(out.contains("| gemini-cli | 8,400 | 16,800 | 0.26* |"));
        assert!(out.contains("21% of this figure is estimated"));
    }

    #[test]
    fn skips_estimate_note_when_zero_share() {
        let lp = pack("english");
        let mut out = String::new();
        render_cost(&mut out, lp, &fixture(0.0));
        assert!(!out.contains("estimated from non-MCP agent activity"));
    }

    #[test]
    fn skips_section_when_empty_and_zero() {
        let lp = pack("english");
        let mut out = String::new();
        let summary = CostSummary {
            from: "x".into(),
            to: "y".into(),
            total_usd: 0.0,
            by_agent: vec![],
            by_model: vec![],
            estimate_share: 0.0,
        };
        render_cost(&mut out, lp, &summary);
        assert!(out.is_empty());
    }

    #[test]
    fn format_usd_uses_micro_decimals_for_tiny_values() {
        assert_eq!(format_usd(0.0001), "0.0001");
        assert_eq!(format_usd(1.234), "1.23");
        assert_eq!(format_usd(1234.5), "1235");
    }

    #[test]
    fn format_int_inserts_thousands_separators() {
        assert_eq!(format_int(0), "0");
        assert_eq!(format_int(123), "123");
        assert_eq!(format_int(12_300), "12,300");
        assert_eq!(format_int(1_234_567), "1,234,567");
    }
}
