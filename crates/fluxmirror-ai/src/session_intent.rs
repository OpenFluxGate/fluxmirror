// Phase 4 M-A3 — session intent decorator.
//
// Sessions are first clustered + named heuristically inside
// `fluxmirror-core::report::sessions`. The studio then pipes the
// resulting `Vec<Session>` through this module to add an LLM-classified
// one-sentence `intent` subtitle per session.
//
// The classification is gated three times so a misconfigured AI layer
// never blocks the heuristic surface:
//
//   1. `config.ai.provider == "off"` — short-circuit, no DB / network.
//   2. No writable `SqliteStore` available — short-circuit (cache + budget
//      both need write access).
//   3. Any error from `synthesise()` — leave `intent = None` for that
//      session and continue with the rest.
//
// Cache key in `synthesise()` is sha256(model + system + user + version).
// The user template ends up structurally identical for the same session
// id (same heuristic name + lifecycle + tool mix + top files + event
// count), so re-running the decorator on the same data is a free cache
// hit per session.
//
// `fluxmirror-core` cannot depend on this crate (would create a cycle:
// ai → core), so the decorator lives here. The studio handlers call
// `synthesise_session_intents` after `collect_sessions` returns.
//
// `fluxmirror-core::report::sessions` clusters and names a session;
// this module then sets `intent` per session.

use serde_json::json;

use fluxmirror_core::config::Config;
use fluxmirror_core::report::dto::Session;
use fluxmirror_store::SqliteStore;

use crate::{synthesise, SynthOptions};

/// Decorate `sessions` in place with LLM-classified intent subtitles.
///
/// `store` may be `None` when the studio has no writable handle on the
/// events DB — that's the same path the off-switch takes (intent stays
/// `None` everywhere).
pub fn synthesise_session_intents(
    store: Option<&SqliteStore>,
    config: &Config,
    sessions: &mut [Session],
) {
    if config.ai.provider == "off" {
        return;
    }
    let Some(store) = store else { return };
    for session in sessions.iter_mut() {
        session.intent = build_intent(store, config, session);
    }
}

/// Build the intent for a single session. Pure helper kept private so
/// the cache-key contract (same session id ⇒ same prompt body) lives
/// in one place.
fn build_intent(store: &SqliteStore, config: &Config, s: &Session) -> Option<String> {
    let ctx = json!({
        "name": s.name,
        "lifecycle": s.lifecycle,
        "tool_mix_json": s.tool_mix,
        "top_files_json": s.top_files,
        "event_count": s.event_count,
    });
    match synthesise(
        store,
        config,
        "session",
        &ctx,
        SynthOptions::for_default_model(config),
    ) {
        Ok(resp) => {
            let text = resp.text.trim().to_string();
            if text.is_empty() {
                None
            } else {
                Some(text)
            }
        }
        Err(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fluxmirror_core::report::dto::SessionLifecycle;

    fn make_session(id: &str) -> Session {
        Session {
            id: id.into(),
            start: "2026-04-26T10:00:00Z".into(),
            end: "2026-04-26T10:30:00Z".into(),
            agents: vec!["claude-code".into()],
            event_count: 12,
            dominant_cwd: Some("/proj/x".into()),
            top_files: vec!["src/lib.rs".into()],
            tool_mix: vec![],
            lifecycle: SessionLifecycle::Building,
            name: "Built: x (Edit-heavy, 1 files)".into(),
            intent: None,
            events: vec![],
        }
    }

    #[test]
    fn off_switch_skips_every_session() {
        let mut cfg = Config::default();
        cfg.ai.provider = "off".into();
        let mut sessions = vec![make_session("a"), make_session("b")];
        synthesise_session_intents(None, &cfg, &mut sessions);
        assert!(sessions.iter().all(|s| s.intent.is_none()));
    }

    #[test]
    fn missing_store_leaves_intent_none() {
        let mut cfg = Config::default();
        cfg.ai.provider = "anthropic".into();
        let mut sessions = vec![make_session("a")];
        // No store handed in — provider is on but we cannot synthesise
        // safely without a cache target, so we no-op gracefully.
        synthesise_session_intents(None, &cfg, &mut sessions);
        assert!(sessions[0].intent.is_none());
    }
}
