// Domain events that flow through fluxmirror.
//
// AgentEvent  — one row per agent tool call (hook source)
// ProxyEvent  — one row per JSON-RPC message line (proxy source)

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::normalize::{ToolClass, ToolKind};

/// Symbolic agent identifier. The wire-format string is kebab-case.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum AgentId {
    ClaudeCode,
    QwenCode,
    GeminiCli,
    ClaudeDesktop,
    Other(String),
}

impl AgentId {
    pub fn as_str(&self) -> &str {
        match self {
            AgentId::ClaudeCode => "claude-code",
            AgentId::QwenCode => "qwen-code",
            AgentId::GeminiCli => "gemini-cli",
            AgentId::ClaudeDesktop => "claude-desktop",
            AgentId::Other(s) => s.as_str(),
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "claude-code" => AgentId::ClaudeCode,
            "qwen-code" => AgentId::QwenCode,
            "gemini-cli" => AgentId::GeminiCli,
            "claude-desktop" => AgentId::ClaudeDesktop,
            other => AgentId::Other(other.to_string()),
        }
    }
}

/// A single agent tool-call observation.
///
/// `tool_raw` is the source-of-truth name as emitted by the CLI
/// (e.g. "Bash", "run_shell_command"). `tool_canonical` and
/// `tool_class` are the post-normalization slots — see normalize.rs.
#[derive(Debug, Clone)]
pub struct AgentEvent {
    pub ts_utc: DateTime<Utc>,
    pub schema_version: u32,
    pub agent: AgentId,
    pub session: String,
    pub tool_raw: String,
    pub tool_canonical: ToolKind,
    pub tool_class: ToolClass,
    pub detail: String,
    pub cwd: PathBuf,
    pub host: String,
    pub user: String,
    pub raw_json: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    C2S,
    S2C,
}

impl Direction {
    pub fn as_str(&self) -> &'static str {
        match self {
            Direction::C2S => "c2s",
            Direction::S2C => "s2c",
        }
    }
}

/// One JSON-RPC line crossing the MCP proxy.
#[derive(Debug, Clone)]
pub struct ProxyEvent {
    pub ts_ms: i64,
    pub direction: Direction,
    pub method: Option<String>,
    pub message_json: String,
    pub server_name: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_id_known_round_trip() {
        for s in ["claude-code", "qwen-code", "gemini-cli", "claude-desktop"] {
            let a = AgentId::from_str(s);
            assert_eq!(a.as_str(), s);
        }
    }

    #[test]
    fn agent_id_other_round_trip() {
        let a = AgentId::from_str("foo-cli");
        assert!(matches!(&a, AgentId::Other(s) if s == "foo-cli"));
        assert_eq!(a.as_str(), "foo-cli");
    }

    #[test]
    fn agent_id_serde_kebab_case() {
        let a = AgentId::ClaudeCode;
        let s = serde_json::to_string(&a).unwrap();
        assert_eq!(s, r#""claude-code""#);
        let back: AgentId = serde_json::from_str(&s).unwrap();
        assert_eq!(back, AgentId::ClaudeCode);
    }

    #[test]
    fn direction_as_str() {
        assert_eq!(Direction::C2S.as_str(), "c2s");
        assert_eq!(Direction::S2C.as_str(), "s2c");
    }
}
