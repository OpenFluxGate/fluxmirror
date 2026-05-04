// fluxmirror-ai: outbound LLM service layer.
//
// One entrypoint — `synthesise()` — wraps every Phase 4 surface that
// asks an LLM to write text. Internally it composes:
//
//   1. prompts::render_prompt   — pull the versioned template + sub vars
//   2. redact_outbound          — M7 secret scrub + $HOME → ~ + length cap
//   3. cache::lookup            — sha256(model, system, user, version) hit?
//   4. budget::check_and_reserve — daily USD ceiling, atomic rename
//   5. Provider::complete       — Anthropic (default) or Ollama (v0.7.1)
//   6. budget::record           — book actual spend
//   7. cache::insert            — persist for the next call
//
// The capture binary (`fluxmirror hook` / `fluxmirror proxy`) does not
// link this crate. Only the studio + report renderers do. Every dep
// added here is therefore confined to the on-demand AI path; the hook
// payload remains the same shape and size as before.

pub mod budget;
pub mod cache;
pub mod prompts;
pub mod provider;
pub mod redact_outbound;
pub mod session_intent;
pub mod types;

use serde_json::Value;

use fluxmirror_core::config::Config;
use fluxmirror_core::redact::from_config;
use fluxmirror_store::SqliteStore;

pub use budget::Budget;
pub use provider::{AnthropicProvider, OllamaProvider, Provider};
pub use redact_outbound::redact_outbound;
pub use session_intent::synthesise_session_intents;
pub use types::{AiError, LlmRequest, LlmResponse};

/// Per-call options for `synthesise()`. Defaults are pulled from the
/// supplied `Config.ai`; callers override per-prompt where needed.
#[derive(Debug, Clone)]
pub struct SynthOptions {
    pub model: String,
    pub max_tokens: u32,
    /// `"anthropic"` or `"ollama"` — `None` means "use config".
    pub provider: Option<&'static str>,
    /// Skip the cache lookup. Cache write still happens on success.
    pub force_refresh: bool,
}

impl Default for SynthOptions {
    fn default() -> Self {
        Self {
            model: String::new(),
            max_tokens: 512,
            provider: None,
            force_refresh: false,
        }
    }
}

impl SynthOptions {
    pub fn for_default_model(cfg: &Config) -> Self {
        Self {
            model: cfg.ai.default_model.clone(),
            max_tokens: 512,
            provider: None,
            force_refresh: false,
        }
    }

    pub fn for_project_model(cfg: &Config) -> Self {
        Self {
            model: cfg.ai.project_model.clone(),
            max_tokens: 768,
            provider: None,
            force_refresh: false,
        }
    }
}

/// One-shot synthesis pipeline.
///
/// `prompt_name` selects a registry entry (daily / session / project /
/// anomaly). `ctx` is a JSON object whose string-coercible fields fill
/// `{placeholder}` markers in the template.
pub fn synthesise(
    store: &SqliteStore,
    config: &Config,
    prompt_name: &str,
    ctx: &Value,
    opts: SynthOptions,
) -> Result<LlmResponse, AiError> {
    // Off-switch fires immediately so callers in heuristic-only mode pay
    // nothing for the cache lookup or budget read.
    if config.ai.provider == "off" {
        return Err(AiError::ProviderNotImplemented);
    }

    let model = if opts.model.is_empty() {
        config.ai.default_model.clone()
    } else {
        opts.model.clone()
    };

    // Layer 1 — render template.
    let (system, user_raw) = prompts::render_prompt(prompt_name, ctx)?;
    let version = prompts::version_of(prompt_name)?;

    // Layer 2 — outbound redaction (M7 scrub + $HOME → ~ + length cap).
    let rules = from_config(config);
    let user = redact_outbound::redact_outbound(&user_raw, &rules, config.ai.max_user_chars);

    // Layer 3 — cache lookup.
    let key = cache::make_cache_key(&model, &system, &user, version);
    if !opts.force_refresh {
        if let Some(hit) = store
            .with_conn(|conn| cache::lookup(conn, &key, config.ai.cache_ttl_days))?
        {
            return Ok(hit);
        }
    }

    // Layer 4 — budget check. Estimate is cheap: 4 chars/token, 1 USD cap
    // means even a 1M-char prompt at $1/1M-tokens stays under cap. The
    // gate here exists to refuse hot loops, not to model real spend.
    let est_usd = estimate_cost(&model, &system, &user, opts.max_tokens);
    let budget = Budget::at_default(config.ai.daily_budget_usd);
    budget.check_and_reserve(est_usd)?;

    // Layer 5 — provider call.
    let req = LlmRequest {
        model: model.clone(),
        system: system.clone(),
        user: user.clone(),
        max_tokens: opts.max_tokens,
        cache_key: key.clone(),
    };
    let chosen = opts.provider.map(|s| s.to_string()).unwrap_or_else(|| {
        config.ai.provider.clone()
    });
    let resp = call_provider(&chosen, &req)?;

    // Layer 6 — book the actual spend.
    budget.record(resp.cost_usd)?;

    // Layer 7 — cache insert.
    store.with_conn(|conn| {
        cache::insert(
            conn,
            &key,
            &resp.text,
            resp.cost_usd,
            &resp.model,
            resp.provider,
        )
    })?;

    Ok(resp)
}

fn call_provider(name: &str, req: &LlmRequest) -> Result<LlmResponse, AiError> {
    match name {
        "anthropic" => match AnthropicProvider::from_env() {
            Some(p) => p.complete(req),
            None => Err(AiError::ProviderUnreachable(
                "ANTHROPIC_API_KEY missing".to_string(),
            )),
        },
        "ollama" => OllamaProvider::from_env().complete(req),
        "off" => Err(AiError::ProviderNotImplemented),
        other => Err(AiError::ProviderUnreachable(format!(
            "unknown provider: {other}"
        ))),
    }
}

/// Coarse cost estimate used by `check_and_reserve`. Deliberately
/// simple — not meant to track real spend, just to gate runaway calls.
fn estimate_cost(model: &str, system: &str, user: &str, max_tokens: u32) -> f64 {
    use fluxmirror_core::cost::{cost_for_usage, lookup, ParsedUsage};
    let p = match lookup(model) {
        Some(p) => p,
        None => return 0.0,
    };
    let chars = (system.len() + user.len()) as f64;
    let input_tok = (chars / 4.0).round() as u64;
    let usage = ParsedUsage {
        input_tokens: input_tok,
        output_tokens: max_tokens as u64,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
    };
    cost_for_usage(p, &usage)
}

