use serde::Serialize;

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
#[derive(Debug, Clone)]
pub enum AgentMessage {
    #[allow(dead_code)]
    System { content: String },
    User { content: UserContent, _attachments: Vec<Attachment> },
    Assistant { content: Option<String>, tool_calls: Vec<ToolCall>, _thinking: Option<String> },
    ToolResult { tool_call_id: String, content: ToolResultContent, _is_error: bool },
}

impl AgentMessage {
    #[allow(dead_code)]
    pub fn system(content: impl Into<String>) -> Self {
        AgentMessage::System { content: content.into() }
    }
    pub fn user(text: impl Into<String>) -> Self {
        AgentMessage::User { content: UserContent::Text(text.into()), _attachments: vec![] }
    }
    #[allow(dead_code)]
    pub fn assistant_text(text: impl Into<String>) -> Self {
        AgentMessage::Assistant { content: Some(text.into()), tool_calls: vec![], _thinking: None }
    }
    pub fn assistant_tool_calls(calls: Vec<ToolCall>) -> Self {
        AgentMessage::Assistant { content: None, tool_calls: calls, _thinking: None }
    }
    pub fn tool_result(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        AgentMessage::ToolResult { tool_call_id: tool_call_id.into(), content: ToolResultContent::Text(content.into()), _is_error: false }
    }
}

#[derive(Debug, Clone)]
pub enum UserContent {
    Text(String),
}

#[derive(Debug, Clone)]
pub enum ToolResultContent {
    Text(String),
}

#[derive(Debug, Clone)]
pub struct Attachment {
    pub _mime_type: String,
    pub _data: Vec<u8>,
    pub _name: String,
}

/// A tool call from the LLM (agent-internal format).
#[derive(Debug, Clone)]
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
}
