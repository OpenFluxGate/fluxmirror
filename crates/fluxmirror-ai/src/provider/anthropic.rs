// Anthropic Messages API provider.
//
// POSTs to `<api_base>/v1/messages` with the canonical
// {model, max_tokens, system, messages: [{role:user, content:str}]}
// shape, parses content[0].text + usage.{input_tokens, output_tokens,
// cache_creation_input_tokens, cache_read_input_tokens}, and computes
// USD via fluxmirror_core::cost::lookup.
//
// `api_base` is overridable so tests can point at mockito. Default is
// `https://api.anthropic.com`.

use std::time::Duration;

use serde_json::{json, Value};

use fluxmirror_core::cost::{cost_for_usage, lookup, ParsedUsage};

use crate::provider::Provider;
use crate::types::{AiError, LlmRequest, LlmResponse};

const DEFAULT_API_BASE: &str = "https://api.anthropic.com";
const ANTHROPIC_VERSION: &str = "2023-06-01";

#[derive(Debug, Clone)]
pub struct AnthropicProvider {
    api_key: String,
    api_base: String,
    timeout: Duration,
}

impl AnthropicProvider {
    /// Build with the env var `ANTHROPIC_API_KEY`. Empty / unset key
    /// returns `None` — callers should fall back to the heuristic path.
    pub fn from_env() -> Option<Self> {
        let key = std::env::var("ANTHROPIC_API_KEY").ok()?;
        if key.trim().is_empty() {
            return None;
        }
        Some(Self {
            api_key: key,
            api_base: std::env::var("FLUXMIRROR_AI_API_BASE")
                .unwrap_or_else(|_| DEFAULT_API_BASE.to_string()),
            timeout: Duration::from_secs(60),
        })
    }

    /// Test/dev constructor with explicit base URL.
    pub fn with_base(api_key: impl Into<String>, api_base: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            api_base: api_base.into(),
            timeout: Duration::from_secs(60),
        }
    }

    fn agent(&self) -> ureq::Agent {
        ureq::AgentBuilder::new().timeout(self.timeout).build()
    }
}

impl Provider for AnthropicProvider {
    fn name(&self) -> &'static str {
        "anthropic"
    }

    fn complete(&self, req: &LlmRequest) -> Result<LlmResponse, AiError> {
        let url = format!("{}/v1/messages", self.api_base.trim_end_matches('/'));
        let body = json!({
            "model": req.model,
            "max_tokens": req.max_tokens,
            "system": req.system,
            "messages": [
                {"role": "user", "content": req.user},
            ],
        });
        let agent = self.agent();
        let resp = agent
            .post(&url)
            .set("x-api-key", &self.api_key)
            .set("anthropic-version", ANTHROPIC_VERSION)
            .set("content-type", "application/json")
            .send_string(&body.to_string());
        let resp = match resp {
            Ok(r) => r,
            Err(ureq::Error::Status(code, r)) => {
                let snippet = r.into_string().unwrap_or_default();
                return Err(AiError::ProviderResponseInvalid(format!(
                    "anthropic http {code}: {}",
                    truncate_err(&snippet)
                )));
            }
            Err(other) => {
                return Err(AiError::ProviderUnreachable(format!("anthropic: {other}")));
            }
        };
        let raw = resp
            .into_string()
            .map_err(|e| AiError::ProviderResponseInvalid(format!("anthropic body: {e}")))?;
        let v: Value = serde_json::from_str(&raw).map_err(|e| {
            AiError::ProviderResponseInvalid(format!("anthropic parse: {e}"))
        })?;
        parse_response(&v, req)
    }
}

/// Pull `text` + `usage` out of a Messages API response and turn it into
/// an `LlmResponse`. Pure function — kept separate so tests can drive it
/// with hand-rolled JSON without spinning up mockito.
pub fn parse_response(v: &Value, req: &LlmRequest) -> Result<LlmResponse, AiError> {
    let text = v
        .get("content")
        .and_then(|c| c.as_array())
        .and_then(|a| a.first())
        .and_then(|first| first.get("text"))
        .and_then(|t| t.as_str())
        .ok_or_else(|| {
            AiError::ProviderResponseInvalid("missing content[0].text".to_string())
        })?
        .to_string();

    let usage = v.get("usage").and_then(|u| u.as_object()).map(|obj| {
        let read_u64 = |k: &str| obj.get(k).and_then(|x| x.as_u64()).unwrap_or(0);
        ParsedUsage {
            input_tokens: read_u64("input_tokens"),
            output_tokens: read_u64("output_tokens"),
            cache_read_tokens: read_u64("cache_read_input_tokens"),
            cache_write_tokens: read_u64("cache_creation_input_tokens"),
        }
    });
    let model = v
        .get("model")
        .and_then(|m| m.as_str())
        .unwrap_or(&req.model)
        .to_string();

    let (cost, tokens_in, tokens_out) = match usage {
        Some(u) => {
            let usd = lookup(&model)
                .map(|p| cost_for_usage(p, &u))
                .unwrap_or(0.0);
            (
                usd,
                clip_u32(u.input_tokens),
                clip_u32(u.output_tokens),
            )
        }
        None => (0.0, 0, 0),
    };

    Ok(LlmResponse {
        text,
        model,
        provider: "anthropic",
        cost_usd: cost,
        tokens_in,
        tokens_out,
        cache_hit: false,
    })
}

fn clip_u32(v: u64) -> u32 {
    if v > u32::MAX as u64 {
        u32::MAX
    } else {
        v as u32
    }
}

fn truncate_err(s: &str) -> String {
    let max = 200;
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let head: String = s.chars().take(max).collect();
        format!("{head}...")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn req() -> LlmRequest {
        LlmRequest {
            model: "claude-haiku-4-5-20251001".into(),
            system: "be terse".into(),
            user: "hello".into(),
            max_tokens: 64,
            cache_key: "deadbeef".into(),
        }
    }

    #[test]
    fn parses_content_and_usage() {
        let v = json!({
            "id": "msg_x",
            "model": "claude-haiku-4-5-20251001",
            "content": [
                {"type": "text", "text": "hi back"}
            ],
            "usage": {
                "input_tokens": 10,
                "output_tokens": 5,
                "cache_creation_input_tokens": 0,
                "cache_read_input_tokens": 0
            }
        });
        let r = parse_response(&v, &req()).expect("ok");
        assert_eq!(r.text, "hi back");
        assert_eq!(r.tokens_in, 10);
        assert_eq!(r.tokens_out, 5);
        assert_eq!(r.provider, "anthropic");
        assert!(r.cost_usd > 0.0);
        assert!(!r.cache_hit);
    }

    #[test]
    fn rejects_missing_content() {
        let v = json!({"model": "claude-haiku-4-5-20251001", "content": []});
        let err = parse_response(&v, &req()).unwrap_err();
        assert!(matches!(err, AiError::ProviderResponseInvalid(_)));
    }
}
