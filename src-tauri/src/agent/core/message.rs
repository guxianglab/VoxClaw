use serde::{Deserialize, Serialize};

use crate::agent::core::request::StopReason;
use crate::agent::core::usage::Usage;

/// Result from a single tool execution, used in agent events.
#[derive(Debug, Clone, Serialize)]
pub struct ToolResult {
    pub call_id: String,
    pub tool_name: String,
    pub content: String,
    pub is_error: bool,
}

/// Summary of a single action performed during agent execution.
#[derive(Debug, Clone, Serialize)]
pub struct ActionSummary {
    pub tool_name: String,
    pub success: bool,
    pub output_preview: String,
}

/// Final result produced by the agent after all turns complete.
#[derive(Debug, Clone, Serialize)]
pub struct AgentResult {
    pub text: String,
    pub actions: Vec<ActionSummary>,
}

/// Agent-internal message type (provider-agnostic).
///
/// Phase 1 cleanup: removed `_`-prefixed placeholder fields. `attachments`,
/// `thinking`, `is_error`, `usage`, `stop_reason` are now real fields so
/// downstream code (session log, compaction, UI) can use them.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "snake_case")]
pub enum AgentMessage {
    #[allow(dead_code)]
    System {
        content: String,
    },
    User {
        content: UserContent,
        attachments: Vec<Attachment>,
    },
    Assistant {
        content: Option<String>,
        tool_calls: Vec<ToolCall>,
        thinking: Option<String>,
        usage: Option<Usage>,
        stop_reason: Option<StopReason>,
    },
    ToolResult {
        tool_call_id: String,
        content: ToolResultContent,
        is_error: bool,
    },
}

impl AgentMessage {
    #[allow(dead_code)]
    pub fn system(content: impl Into<String>) -> Self {
        AgentMessage::System {
            content: content.into(),
        }
    }
    pub fn user(text: impl Into<String>) -> Self {
        AgentMessage::User {
            content: UserContent::Text(text.into()),
            attachments: vec![],
        }
    }
    #[allow(dead_code)]
    pub fn assistant_text(text: impl Into<String>) -> Self {
        AgentMessage::Assistant {
            content: Some(text.into()),
            tool_calls: vec![],
            thinking: None,
            usage: None,
            stop_reason: None,
        }
    }
    pub fn assistant_tool_calls(calls: Vec<ToolCall>) -> Self {
        AgentMessage::Assistant {
            content: None,
            tool_calls: calls,
            thinking: None,
            usage: None,
            stop_reason: None,
        }
    }
    pub fn tool_result(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        AgentMessage::ToolResult {
            tool_call_id: tool_call_id.into(),
            content: ToolResultContent::Text(content.into()),
            is_error: false,
        }
    }

    /// Construct a tool result with explicit error flag.
    #[allow(dead_code)]
    pub fn tool_result_with_error(
        tool_call_id: impl Into<String>,
        content: impl Into<String>,
        is_error: bool,
    ) -> Self {
        AgentMessage::ToolResult {
            tool_call_id: tool_call_id.into(),
            content: ToolResultContent::Text(content.into()),
            is_error,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UserContent {
    Text(String),
    /// Inline image attached as part of the user message.
    /// Reserved for future multimodal input; no provider currently consumes it.
    #[allow(dead_code)]
    Image { mime_type: String, data: Vec<u8> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolResultContent {
    Text(String),
    /// Image returned by a tool (e.g. screenshot). Surfaced in the UI; not
    /// yet sent back to the LLM (Phase 6 will wire vision-capable providers).
    #[allow(dead_code)]
    Image { mime_type: String, data: Vec<u8> },
}

/// Out-of-band attachment carried with a user message.
/// Currently informational; turned into provider payloads in Phase 4+.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct Attachment {
    pub mime_type: String,
    pub data: Vec<u8>,
    pub name: String,
}

/// A tool call from the LLM (agent-internal format).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

/// Default system prompt for the agent.
pub fn default_system_prompt() -> &'static str {
    "You are VoxClaw Agent, a voice-driven AI assistant. The user speaks to you via voice input.\n\
     - If the user wants to dictate text, return it directly without using tools.\n\
     - If the user wants to perform an action (open app, run command, etc.), use the appropriate tool.\n\
     - Be concise — voice interactions should be quick.\n\
     - Respond in the same language the user speaks.\n\
     - When you perform an action, briefly confirm what you did."
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_result_construction() {
        let result = ToolResult {
            call_id: "call_1".into(),
            tool_name: "bash".into(),
            content: "output".into(),
            is_error: false,
        };
        assert!(!result.is_error);
    }

    #[test]
    fn agent_result_construction() {
        let result = AgentResult {
            text: "final answer".into(),
            actions: vec![],
        };
        assert_eq!(result.text, "final answer");
    }

    #[test]
    fn action_summary_construction() {
        let summary = ActionSummary {
            tool_name: "execute_command".into(),
            success: true,
            output_preview: "Opened calculator".into(),
        };
        assert!(summary.success);
        assert_eq!(summary.tool_name, "execute_command");
    }

    #[test]
    fn default_system_prompt_returns_non_empty() {
        let prompt = default_system_prompt();
        assert!(!prompt.is_empty());
        assert!(prompt.contains("VoxClaw Agent"));
    }

    #[test]
    fn tool_result_serializes() {
        let result = ToolResult {
            call_id: "call_1".into(),
            tool_name: "bash".into(),
            content: "output".into(),
            is_error: false,
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains(r#""call_id":"call_1""#));
        assert!(json.contains(r#""tool_name":"bash""#));
        assert!(json.contains(r#""is_error":false"#));
    }

    #[test]
    fn agent_result_serializes() {
        let result = AgentResult {
            text: "done".into(),
            actions: vec![
                ActionSummary {
                    tool_name: "file_read".into(),
                    success: true,
                    output_preview: "file contents...".into(),
                },
            ],
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains(r#""text":"done""#));
        assert!(json.contains("file_read"));
    }

    #[test]
    fn tool_result_message_with_error() {
        let msg = AgentMessage::tool_result_with_error("call_1", "boom", true);
        match msg {
            AgentMessage::ToolResult { is_error, .. } => assert!(is_error),
            _ => panic!("expected ToolResult"),
        }
    }

    #[test]
    fn assistant_message_carries_usage_and_stop_reason() {
        let msg = AgentMessage::Assistant {
            content: Some("hi".into()),
            tool_calls: vec![],
            thinking: Some("...".into()),
            usage: Some(Usage {
                input: 5,
                output: 7,
                ..Default::default()
            }),
            stop_reason: Some(StopReason::Stop),
        };
        match msg {
            AgentMessage::Assistant {
                usage, stop_reason, thinking, ..
            } => {
                assert_eq!(usage.unwrap().output, 7);
                assert_eq!(stop_reason, Some(StopReason::Stop));
                assert_eq!(thinking.as_deref(), Some("..."));
            }
            _ => panic!("expected Assistant"),
        }
    }
}
