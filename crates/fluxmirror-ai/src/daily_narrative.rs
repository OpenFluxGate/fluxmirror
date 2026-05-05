// Phase 4 M-A2 — daily narrative decorator.
//
// `fluxmirror_core::report::data::collect_today` returns a `TodayData`
// snapshot of the current day's traffic. This module turns that
// snapshot into a `DailyNarrative` — either an LLM-written paragraph
// via the `daily` prompt template, or a deterministic fallback from
// `fluxmirror_core::report::ai_narrative::heuristic_paragraph` when the
// AI provider isn't reachable.
//
// Lives in this crate (not `fluxmirror-core`) for the same reason
// `session_intent` and `anomaly_story` do: `synthesise()` itself depends
// on the budget / cache / provider modules in `fluxmirror-ai`, so
// pushing the wrapper down to `core` would re-introduce a cycle.
//
// The fallback in `core` is the failure-mode contract — every leg of
// this pipeline (off switch, missing store, budget hit, network error,
// JSON parse error) ends up calling `heuristic_paragraph()` so the
// narrative field is always populated for downstream surfaces.

use fluxmirror_core::config::Config;
use fluxmirror_core::report::ai_narrative::heuristic_paragraph;
use fluxmirror_core::report::data::build_daily_summary_input;
use fluxmirror_core::report::dto::{DailyNarrative, NarrativeSource, TodayData};
use fluxmirror_store::SqliteStore;

use crate::{synthesise, SynthOptions};

/// Build a [`DailyNarrative`] for `today`. The contract is that this
/// function never panics and always returns a populated paragraph —
/// every error leg drops into the heuristic fallback.
///
/// `store` is `None` whenever the studio runs without a writable store
/// handle (the tests + the `provider="off"` path both take this branch).
pub fn synthesise_daily(
    store: Option<&SqliteStore>,
    config: &Config,
    today: &TodayData,
) -> DailyNarrative {
    // Off-switch first — saves a JSON build + a mutex acquisition.
    if config.ai.provider == "off" {
        return heuristic(today);
    }
    // No writable store ⇒ cache + budget can't be consulted. Fall back
    // rather than skip those layers and call the provider directly:
    // the privacy / cost contract is that every outbound call goes
    // through `synthesise()`.
    let Some(store) = store else {
        return heuristic(today);
    };
    // Empty windows have nothing useful to say — bypass the LLM,
    // returning the deterministic empty-day paragraph straight away
    // so we don't burn budget on prompts the model can only echo.
    if today.total_events == 0 {
        return heuristic(today);
    }

    let ctx = build_daily_summary_input(today);
    let opts = SynthOptions::for_default_model(config);
    match synthesise(store, config, "daily", &ctx, opts) {
        Ok(resp) => {
            let text = resp.text.trim().to_string();
            if text.is_empty() {
                heuristic(today)
            } else {
                DailyNarrative {
                    paragraph: text,
                    source: NarrativeSource::Llm,
                    model: Some(resp.model),
                    cost_usd: resp.cost_usd,
                    cache_hit: resp.cache_hit,
                }
            }
        }
        Err(_) => heuristic(today),
    }
}

fn heuristic(today: &TodayData) -> DailyNarrative {
    DailyNarrative {
        paragraph: heuristic_paragraph(today),
        source: NarrativeSource::Heuristic,
        model: None,
        cost_usd: 0.0,
        cache_hit: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn busy_today() -> TodayData {
        TodayData {
            date: NaiveDate::from_ymd_opt(2026, 4, 26).unwrap(),
            tz: "UTC".to_string(),
            total_events: 12,
            writes_total: 4,
            reads_total: 5,
            ..Default::default()
        }
    }

    #[test]
    fn off_switch_returns_heuristic_with_non_empty_paragraph() {
        let mut cfg = Config::default();
        cfg.ai.provider = "off".into();
        let n = synthesise_daily(None, &cfg, &busy_today());
        assert_eq!(n.source, NarrativeSource::Heuristic);
        assert!(!n.paragraph.is_empty());
        assert!(n.model.is_none());
        assert_eq!(n.cost_usd, 0.0);
        assert!(!n.cache_hit);
    }

    #[test]
    fn missing_store_falls_back_to_heuristic_even_when_provider_on() {
        let mut cfg = Config::default();
        cfg.ai.provider = "anthropic".into();
        let n = synthesise_daily(None, &cfg, &busy_today());
        assert_eq!(n.source, NarrativeSource::Heuristic);
        assert!(!n.paragraph.is_empty());
    }

    #[test]
    fn empty_window_short_circuits_to_heuristic_even_with_provider_on() {
        let mut cfg = Config::default();
        cfg.ai.provider = "anthropic".into();
        let mut t = busy_today();
        t.total_events = 0;
        t.writes_total = 0;
        t.reads_total = 0;
        let n = synthesise_daily(None, &cfg, &t);
        assert_eq!(n.source, NarrativeSource::Heuristic);
        assert!(n.paragraph.to_lowercase().contains("no agent activity"));
    }
}
