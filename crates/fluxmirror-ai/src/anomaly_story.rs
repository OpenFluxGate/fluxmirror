// Phase 4 M-A6 — anomaly story decorator.
//
// Heuristic detections in `fluxmirror_core::report::anomaly` produce
// raw figures (kind, observed, baseline, evidence). This module
// wraps each detection with a one-sentence story, either via the
// `anomaly` prompt template or — on any LLM failure — a deterministic
// fallback so the studio API never needs to special-case "no AI" paths.
//
// Lives in this crate (not `fluxmirror-core`) because `synthesise()`
// itself depends on the budget, cache, and provider modules; pushing
// the wrapper down to `core` would re-introduce the cycle the
// session-intent decorator already solved by living here.
//
// `fluxmirror-core::report::anomaly` produces detections; this module
// then turns each into an `AnomalyStory`.

use serde_json::json;

use fluxmirror_core::config::Config;
use fluxmirror_core::report::anomaly::AnomalyDetection;
use fluxmirror_core::report::dto::{AnomalyKind, AnomalySource, AnomalyStory};
use fluxmirror_store::SqliteStore;

use crate::{synthesise, SynthOptions};

/// Wrap one heuristic detection with an LLM- or template-generated
/// one-sentence story. `store = None` (or `provider == "off"`) takes
/// the heuristic branch immediately; any error from `synthesise()` also
/// falls through to the heuristic so the API never panics on a flaky
/// provider.
pub fn synthesise_anomaly(
    store: Option<&SqliteStore>,
    config: &Config,
    detection: &AnomalyDetection,
) -> AnomalyStory {
    let llm = if config.ai.provider == "off" {
        None
    } else {
        store.and_then(|s| call_llm(s, config, detection))
    };
    match llm {
        Some(text) => AnomalyStory {
            kind: detection.kind,
            story: text,
            observed: detection.observed,
            baseline: detection.baseline,
            evidence: detection.evidence.clone(),
            source: AnomalySource::Llm,
        },
        None => AnomalyStory {
            kind: detection.kind,
            story: heuristic_story(detection),
            observed: detection.observed,
            baseline: detection.baseline,
            evidence: detection.evidence.clone(),
            source: AnomalySource::Heuristic,
        },
    }
}

fn call_llm(
    store: &SqliteStore,
    config: &Config,
    detection: &AnomalyDetection,
) -> Option<String> {
    let ctx = json!({
        "kind": kind_label(detection.kind),
        "anomaly_kind": kind_label(detection.kind),
        "observed": format!("{:.2}", detection.observed),
        "baseline": format!("{:.2}", detection.baseline),
        "evidence_json": serde_json::to_string(&detection.evidence)
            .unwrap_or_else(|_| "[]".into()),
        "anomaly_window": "today",
        "agent": detection.evidence.first().cloned().unwrap_or_default(),
        "recent_context": detection.evidence.join(" | "),
    });
    let resp = synthesise(
        store,
        config,
        "anomaly",
        &ctx,
        SynthOptions::for_default_model(config),
    )
    .ok()?;
    let text = resp.text.trim();
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}

/// Deterministic story used when the LLM path is unreachable.
fn heuristic_story(d: &AnomalyDetection) -> String {
    let head = d.evidence.first().map(String::as_str).unwrap_or("");
    match d.kind {
        AnomalyKind::FileEditSpike => {
            if head.is_empty() {
                format!(
                    "Edit spike detected ({:.0}× the rolling avg of {:.1}).",
                    d.observed, d.baseline
                )
            } else {
                format!(
                    "{head}. That's {:.0}× the rolling avg of {:.1}.",
                    d.observed, d.baseline
                )
            }
        }
        AnomalyKind::ToolMixDeparture => format!(
            "Tool mix shifted (cosine distance {:.2} ≥ {:.2}).",
            d.observed, d.baseline
        ),
        AnomalyKind::NewAgent => {
            if head.is_empty() {
                "A new agent appeared in today's traffic.".to_string()
            } else {
                format!("New agent active today: {head}.")
            }
        }
        AnomalyKind::NewMcpMethod => {
            if head.is_empty() {
                "A new MCP method appeared in today's traffic.".to_string()
            } else {
                format!("New MCP method called today: {head}.")
            }
        }
        AnomalyKind::CostPerCallRise => format!(
            "Avg cost-per-call rose to ${:.5} (vs baseline ${:.5}).",
            d.observed, d.baseline
        ),
    }
}

/// Stable lowercase label sent to the LLM template. Kept here (not on
/// the `AnomalyKind` enum) because the JSON serde shape is `snake_case`
/// and this label set is the one the prompt expects.
fn kind_label(kind: AnomalyKind) -> &'static str {
    match kind {
        AnomalyKind::FileEditSpike => "file_edit_spike",
        AnomalyKind::ToolMixDeparture => "tool_mix_departure",
        AnomalyKind::NewAgent => "new_agent",
        AnomalyKind::NewMcpMethod => "new_mcp_method",
        AnomalyKind::CostPerCallRise => "cost_per_call_rise",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn det(kind: AnomalyKind) -> AnomalyDetection {
        AnomalyDetection {
            kind,
            observed: 14.0,
            baseline: 2.0,
            evidence: vec!["Cargo.toml +14 edits (rolling avg 2.0)".into()],
        }
    }

    #[test]
    fn provider_off_yields_heuristic_source() {
        let mut cfg = Config::default();
        cfg.ai.provider = "off".into();
        let story = synthesise_anomaly(None, &cfg, &det(AnomalyKind::FileEditSpike));
        assert_eq!(story.source, AnomalySource::Heuristic);
        assert!(!story.story.is_empty());
        assert_eq!(story.kind, AnomalyKind::FileEditSpike);
    }

    #[test]
    fn missing_store_falls_back_to_heuristic_even_when_provider_on() {
        let mut cfg = Config::default();
        cfg.ai.provider = "anthropic".into();
        let story = synthesise_anomaly(None, &cfg, &det(AnomalyKind::NewAgent));
        assert_eq!(story.source, AnomalySource::Heuristic);
        assert!(story.story.contains("Cargo.toml") || story.story.contains("agent"));
    }

    #[test]
    fn heuristic_story_covers_every_kind() {
        for kind in [
            AnomalyKind::FileEditSpike,
            AnomalyKind::ToolMixDeparture,
            AnomalyKind::NewAgent,
            AnomalyKind::NewMcpMethod,
            AnomalyKind::CostPerCallRise,
        ] {
            let story = heuristic_story(&det(kind));
            assert!(!story.is_empty(), "empty heuristic story for {kind:?}");
        }
    }
}
