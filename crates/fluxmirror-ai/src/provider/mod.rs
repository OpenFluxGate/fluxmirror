// Provider abstraction.
//
// Two implementations:
//
//   - AnthropicProvider — POSTs to /v1/messages with x-api-key from
//     ANTHROPIC_API_KEY. Parses content[0].text + the usage block,
//     computes cost via fluxmirror_core::cost::lookup.
//   - OllamaProvider    — stub for v0.7.0. Pings /api/generate; if the
//     server is unreachable, every call returns ProviderNotImplemented.
//     Full impl lands in v0.7.1.

pub mod anthropic;
pub mod ollama;

use crate::types::{AiError, LlmRequest, LlmResponse};

/// Outbound provider interface.
pub trait Provider {
    fn complete(&self, req: &LlmRequest) -> Result<LlmResponse, AiError>;
    fn name(&self) -> &'static str;
}

pub use anthropic::AnthropicProvider;
pub use ollama::OllamaProvider;
