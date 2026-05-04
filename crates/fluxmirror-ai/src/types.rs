// Public request / response shapes + crate error type.

use serde::{Deserialize, Serialize};

/// One outbound request to a provider. The `cache_key` field is a hex
/// digest pre-computed by the caller (or by `synthesise()`); it travels
/// with the request so the cache layer doesn't have to recompute it on
/// the post-call insert.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmRequest {
    pub model: String,
    pub system: String,
    pub user: String,
    pub max_tokens: u32,
    pub cache_key: String,
}

/// One synthesised response, after all of: cache lookup, redaction,
/// budget reserve, provider call, budget record, cache insert.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmResponse {
    pub text: String,
    pub model: String,
    pub provider: &'static str,
    pub cost_usd: f64,
    pub tokens_in: u32,
    pub tokens_out: u32,
    pub cache_hit: bool,
}

/// Crate-wide error type.
#[derive(Debug)]
pub enum AiError {
    /// Daily USD ceiling already met; caller should fall back to a
    /// heuristic (or skip).
    BudgetExceeded,
    /// Network unreachable / DNS failure / connection refused.
    ProviderUnreachable(String),
    /// Provider responded but the body didn't match the expected shape.
    ProviderResponseInvalid(String),
    /// Provider configured but not implemented yet (Ollama path in v0.7.0,
    /// or `provider = "off"`).
    ProviderNotImplemented,
    /// Persisted-cache backing store error.
    Storage(rusqlite::Error),
    /// File / dir IO at the budget or cache layer.
    Io(std::io::Error),
    /// Prompt registry / context substitution failure.
    Prompt(String),
}

impl std::fmt::Display for AiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AiError::BudgetExceeded => write!(f, "ai budget exceeded for today"),
            AiError::ProviderUnreachable(s) => write!(f, "ai provider unreachable: {s}"),
            AiError::ProviderResponseInvalid(s) => {
                write!(f, "ai provider response invalid: {s}")
            }
            AiError::ProviderNotImplemented => write!(f, "ai provider not implemented"),
            AiError::Storage(e) => write!(f, "ai storage error: {e}"),
            AiError::Io(e) => write!(f, "ai io error: {e}"),
            AiError::Prompt(s) => write!(f, "ai prompt error: {s}"),
        }
    }
}

impl std::error::Error for AiError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            AiError::Storage(e) => Some(e),
            AiError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<rusqlite::Error> for AiError {
    fn from(e: rusqlite::Error) -> Self {
        AiError::Storage(e)
    }
}

impl From<std::io::Error> for AiError {
    fn from(e: std::io::Error) -> Self {
        AiError::Io(e)
    }
}
