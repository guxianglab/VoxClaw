use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::pin::Pin;

use futures_util::Stream;

use crate::agent::error::AgentError;

// --- Thinking/Reasoning ---

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Copy)]
#[serde(rename_all = "lowercase")]
pub enum ThinkingLevel {
    /// No thinking. Serialized as "none"; "off" is also accepted on input.
    None,
    Minimal,
    Low,
    Medium,
    High,
    XHigh,
}

impl ThinkingLevel {
    pub fn from_str_lossy(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "minimal" => ThinkingLevel::Minimal,
            "low" => ThinkingLevel::Low,
            "medium" => ThinkingLevel::Medium,
            "high" => ThinkingLevel::High,
            "xhigh" | "x-high" | "xhi" => ThinkingLevel::XHigh,
            "off" | "none" | "" => ThinkingLevel::None,
            _ => ThinkingLevel::None,
        }
    }

    /// True for any level above `None`.
    pub fn is_active(self) -> bool {
        !matches!(self, ThinkingLevel::None)
    }
}

/// Per-level token budgets for providers that take a numeric thinking budget
/// (Anthropic extended thinking, Gemini thinkingConfig, etc.).
///
/// Defaults mirror pi-mono's `DEFAULT_THINKING_BUDGETS`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ThinkingBudgets {
    pub minimal: u32,
    pub low: u32,
    pub medium: u32,
    pub high: u32,
    pub xhigh: u32,
}

impl Default for ThinkingBudgets {
    fn default() -> Self {
        Self {
            minimal: 128,
            low: 512,
            medium: 1024,
            high: 2048,
            xhigh: 4096,
        }
    }
}

impl ThinkingBudgets {
    pub fn budget_for(self, level: ThinkingLevel) -> Option<u32> {
        match level {
            ThinkingLevel::None => None,
            ThinkingLevel::Minimal => Some(self.minimal),
            ThinkingLevel::Low => Some(self.low),
            ThinkingLevel::Medium => Some(self.medium),
            ThinkingLevel::High => Some(self.high),
            ThinkingLevel::XHigh => Some(self.xhigh),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThinkingConfig {
    pub level: ThinkingLevel,
    /// Explicit token budget; if `None`, providers fall back to
    /// `ThinkingBudgets::budget_for(level)`.
    pub budget_tokens: Option<u32>,
}

// --- Stop Reason ---

/// Why an assistant turn ended.
///
/// Aligns with pi-mono's `StopReason`. `Cancelled` is serialized as
/// `"aborted"` and `MaxTokens` as `"length"` to match the JS surface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    Stop,
    ToolCalls,
    #[serde(rename = "length")]
    MaxTokens,
    #[serde(rename = "aborted")]
    Cancelled,
    Refusal,
    Error(String),
}

impl StopReason {
    pub fn from_openai(finish_reason: &str) -> Self {
        match finish_reason {
            "stop" => StopReason::Stop,
            "tool_calls" | "function_call" => StopReason::ToolCalls,
            "length" => StopReason::MaxTokens,
            "content_filter" | "refusal" => StopReason::Refusal,
            other => StopReason::Error(other.to_string()),
        }
    }
}

// --- LLM Messages (provider-specific format) ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmMessage {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<LlmToolCall>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl LlmMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".into(),
            content: Some(content.into()),
            tool_calls: vec![],
            tool_call_id: None,
        }
    }
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".into(),
            content: Some(content.into()),
            tool_calls: vec![],
            tool_call_id: None,
        }
    }
    pub fn assistant_text(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".into(),
            content: Some(content.into()),
            tool_calls: vec![],
            tool_call_id: None,
        }
    }
    pub fn assistant_tool_calls(calls: Vec<LlmToolCall>) -> Self {
        Self {
            role: "assistant".into(),
            content: None,
            tool_calls: calls,
            tool_call_id: None,
        }
    }
    pub fn tool_result(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: "tool".into(),
            content: Some(content.into()),
            tool_calls: vec![],
            tool_call_id: Some(tool_call_id.into()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: LlmFunctionCall,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmFunctionCall {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LlmTool {
    #[serde(rename = "type")]
    pub spec_type: String,
    pub function: LlmToolFunction,
}

impl LlmTool {
    pub fn new(name: impl Into<String>, description: impl Into<String>, parameters: Value) -> Self {
        Self {
            spec_type: "function".into(),
            function: LlmToolFunction {
                name: name.into(),
                description: description.into(),
                parameters,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct LlmToolFunction {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

// --- Request ---

#[derive(Debug, Clone)]
pub struct LlmRequest {
    pub model: String,
    pub messages: Vec<LlmMessage>,
    pub tools: Vec<LlmTool>,
    pub thinking: Option<ThinkingConfig>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
}

// --- Stream Event (no Error variant - errors go in Result wrapper) ---

#[derive(Debug, Clone)]
pub enum LlmStreamEvent {
    TextDelta(String),
    ThinkingDelta(String),
    ToolCallStart {
        index: usize,
        id: String,
        name: String,
    },
    ToolCallDelta {
        index: usize,
        arguments_delta: String,
    },
    ToolCallEnd {
        index: usize,
    },
    /// Token usage and cost reported by the provider for this assistant
    /// response. Emitted before `Done` when available.
    Usage(crate::agent::core::usage::Usage),
    Done {
        stop_reason: StopReason,
    },
}

/// Stream type alias: pinned boxed stream of Result<LlmStreamEvent, AgentError>
pub type LlmStream = Pin<Box<dyn Stream<Item = Result<LlmStreamEvent, AgentError>> + Send>>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thinking_config_level_serializes() {
        let config = ThinkingConfig {
            level: ThinkingLevel::Medium,
            budget_tokens: Some(8000),
        };
        let json = serde_json::to_string(&config.level).unwrap();
        assert_eq!(json, "\"medium\"");
    }

    #[test]
    fn stop_reason_from_openai() {
        let sr = StopReason::from_openai("stop");
        assert!(matches!(sr, StopReason::Stop));
        let sr = StopReason::from_openai("tool_calls");
        assert!(matches!(sr, StopReason::ToolCalls));
        let sr = StopReason::from_openai("length");
        assert!(matches!(sr, StopReason::MaxTokens));
    }

    #[test]
    fn llm_message_construction() {
        let msg = LlmMessage::system("You are helpful.");
        assert_eq!(msg.role, "system");
        let msg = LlmMessage::user("Hello");
        assert_eq!(msg.role, "user");
        let msg = LlmMessage::assistant_text("Hi there");
        assert_eq!(msg.role, "assistant");
    }
}
