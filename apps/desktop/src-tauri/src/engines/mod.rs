//! The vendor-neutral Engine trait and its adapters (docs/PLAN.md 6.2). No UI
//! or platform code. Vendor specifics live only in the adapter files and in
//! `config/models.json`.

pub mod anthropic_api;
mod sse;

use std::fmt;
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// How hard to think. Users pick an effort, never a model (docs/PLAN.md 6.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Effort {
    Low,
    Medium,
    High,
    Max,
}

impl Effort {
    #[cfg(test)]
    pub const ALL: [Effort; 4] = [Effort::Low, Effort::Medium, Effort::High, Effort::Max];

    /// One level up, if there is one and it's within `ceiling`.
    pub fn deeper(self, ceiling: Effort) -> Option<Effort> {
        let next = match self {
            Effort::Low => Effort::Medium,
            Effort::Medium => Effort::High,
            Effort::High => Effort::Max,
            Effort::Max => return None,
        };
        (next <= ceiling).then_some(next)
    }

    pub fn key(self) -> &'static str {
        match self {
            Effort::Low => "low",
            Effort::Medium => "medium",
            Effort::High => "high",
            Effort::Max => "max",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

/// One piece of a message, in a form every adapter can convert.
#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    Text(String),
    Image {
        media_type: String,
        data: Vec<u8>,
    },
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    ToolResult {
        id: String,
        content: String,
        is_error: bool,
    },
    /// A block only its engine understands (reasoning, server-side tool
    /// results). That engine sends it back unchanged; others drop it.
    Opaque {
        engine: &'static str,
        data: serde_json::Value,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Message {
    pub role: Role,
    pub content: Vec<Block>,
}

impl Message {
    pub fn user(content: Vec<Block>) -> Self {
        Self {
            role: Role::User,
            content,
        }
    }

    pub fn user_text(text: impl Into<String>) -> Self {
        Self::user(vec![Block::Text(text.into())])
    }

    /// The message's plain text, joined.
    #[cfg(test)]
    pub fn text(&self) -> String {
        self.content
            .iter()
            .filter_map(|b| match b {
                Block::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect()
    }

    pub fn tool_uses(&self) -> impl Iterator<Item = (&str, &str, &serde_json::Value)> {
        self.content.iter().filter_map(|b| match b {
            Block::ToolUse { id, name, input } => Some((id.as_str(), name.as_str(), input)),
            _ => None,
        })
    }
}

/// A local tool the orchestrator runs (docs/PLAN.md 7), described with JSON Schema.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

pub struct AskRequest {
    pub effort: Effort,
    pub system: String,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolSpec>,
    /// Let the engine search the web with its own search, if it has one.
    pub web_search: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum EngineEvent {
    TextDelta(String),
    /// What the engine is doing, for the status line ("Thinking…"), or None
    /// to clear it.
    Status(Option<String>),
    ToolStarted {
        name: String,
        summary: String,
    },
    ToolFinished {
        name: String,
    },
    Usage(Usage),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    /// Estimated, when the engine knows its prices.
    pub cost_usd: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopReason {
    EndTurn,
    /// Wants local tools run (see `Message::tool_uses`).
    ToolUse,
    MaxTokens,
    /// A server-side tool loop paused; send the conversation again to resume.
    PauseTurn,
    Refusal,
    Other,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AskOutcome {
    pub message: Message,
    pub stop: StopReason,
    pub usage: Usage,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EngineError {
    /// No key yet, or the engine isn't installed.
    NotConfigured,
    InvalidKey,
    NoCredits,
    RateLimited {
        retry_after: Option<Duration>,
    },
    Overloaded,
    Offline(String),
    Cancelled,
    /// Anything else the service said no to.
    Api {
        status: u16,
        message: String,
    },
    /// A response we couldn't make sense of.
    Protocol(String),
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotConfigured => write!(f, "The AI engine isn't set up yet."),
            Self::InvalidKey => write!(f, "The API key was rejected."),
            Self::NoCredits => write!(f, "The account is out of credits."),
            Self::RateLimited { .. } => write!(f, "Too many requests right now."),
            Self::Overloaded => write!(f, "The AI service is overloaded."),
            Self::Offline(detail) => write!(f, "Couldn't reach the AI service ({detail})."),
            Self::Cancelled => write!(f, "Cancelled."),
            Self::Api { status, message } => {
                write!(f, "The AI service said no ({status}): {message}")
            }
            Self::Protocol(detail) => write!(f, "Unexpected response: {detail}"),
        }
    }
}

impl std::error::Error for EngineError {}

/// Whether an engine can answer right now, for Settings and onboarding.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum Readiness {
    Ready,
    NeedsSetup { reason: String },
}

/// How Settings presents an engine's credential, so the UI needn't know any
/// vendor.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub key_label: &'static str,
    pub key_help: &'static str,
    pub key_help_url: &'static str,
}

/// Per-effort limits the orchestrator enforces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Local tool calls per answer.
    pub tool_budget: u32,
}

#[async_trait]
pub trait Engine: Send + Sync {
    fn info(&self) -> EngineInfo;

    fn limits(&self, effort: Effort) -> Limits;

    async fn check_ready(&self) -> Readiness;

    /// Streams one model response. Events go to `events` as they arrive; the
    /// full message comes back at the end.
    async fn ask(
        &self,
        req: AskRequest,
        events: mpsc::UnboundedSender<EngineEvent>,
        cancel: CancellationToken,
    ) -> Result<AskOutcome, EngineError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn going_deeper_steps_up_to_the_ceiling() {
        assert_eq!(Effort::Low.deeper(Effort::High), Some(Effort::Medium));
        assert_eq!(Effort::Medium.deeper(Effort::High), Some(Effort::High));
        assert_eq!(
            Effort::High.deeper(Effort::High),
            None,
            "Max only by choice"
        );
        assert_eq!(Effort::High.deeper(Effort::Max), Some(Effort::Max));
        assert_eq!(Effort::Max.deeper(Effort::Max), None);
        assert_eq!(Effort::Low.deeper(Effort::Low), None);
    }
}
