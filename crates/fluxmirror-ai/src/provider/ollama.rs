// Ollama provider — stub for v0.7.0.
//
// In v0.7.0 the Ollama path is intentionally minimal: we ping the
// configured endpoint at construction; if the server is unreachable,
// every `complete()` call returns `ProviderNotImplemented` so callers
// can take the heuristic fallback. Once a server is reachable in
// v0.7.1, this same struct grows the full /api/generate impl.
//
// Local-only by default — `http://localhost:11434/api/generate`.

use std::time::Duration;

use crate::provider::Provider;
use crate::types::{AiError, LlmRequest, LlmResponse};

const DEFAULT_BASE: &str = "http://localhost:11434";

#[derive(Debug, Clone)]
pub struct OllamaProvider {
    base: String,
    timeout: Duration,
}

impl OllamaProvider {
    pub fn from_env() -> Self {
        let base = std::env::var("FLUXMIRROR_OLLAMA_BASE")
            .unwrap_or_else(|_| DEFAULT_BASE.to_string());
        Self {
            base,
            timeout: Duration::from_secs(30),
        }
    }

    pub fn with_base(base: impl Into<String>) -> Self {
        Self {
            base: base.into(),
            timeout: Duration::from_secs(30),
        }
    }

    /// Cheap reachability probe. Used by `complete()` so we can return
    /// a clean `ProviderUnreachable` instead of a long ureq error chain.
    pub fn is_reachable(&self) -> bool {
        let url = format!("{}/api/tags", self.base.trim_end_matches('/'));
        let agent = ureq::AgentBuilder::new().timeout(self.timeout).build();
        agent.get(&url).call().is_ok()
    }
}

impl Provider for OllamaProvider {
    fn name(&self) -> &'static str {
        "ollama"
    }

    fn complete(&self, _req: &LlmRequest) -> Result<LlmResponse, AiError> {
        // v0.7.0 stub: no live calls. The reachability probe still runs
        // so the failure mode is "unreachable" when there's no server,
        // and "not implemented" once a server is up — that distinction
        // makes the v0.7.1 graduation easy to spot in logs.
        if !self.is_reachable() {
            return Err(AiError::ProviderUnreachable(format!(
                "ollama at {} is not reachable",
                self.base
            )));
        }
        Err(AiError::ProviderNotImplemented)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unreachable_path_returns_clean_error() {
        // 127.0.0.1:1 is reserved-port-territory — the OS will refuse
        // before any handshake. Asserts the error is the unreachable
        // variant, not a panic / not the not-implemented stub.
        let p = OllamaProvider::with_base("http://127.0.0.1:1");
        let req = LlmRequest {
            model: "llama3".into(),
            system: "be terse".into(),
            user: "hi".into(),
            max_tokens: 32,
            cache_key: "k".into(),
        };
        match p.complete(&req) {
            Err(AiError::ProviderUnreachable(_)) => (),
            other => panic!("expected unreachable, got {other:?}"),
        }
    }
}
