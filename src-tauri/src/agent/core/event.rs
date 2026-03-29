use serde::Serialize;

use crate::agent::core::message::{AgentResult, ToolResult};
use crate::agent::core::request::StopReason;

/// High-level events emitted by the agent for UI consumption.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    AgentStart,
    AgentEnd { result: AgentResult },
    TurnStart { turn: u32 },
    TurnEnd { turn: u32 },
    MessageStart,
    MessageUpdate { content: MessageUpdateContent },
    MessageEnd { stop_reason: StopReason },
    ToolExecutionStart { tool_name: String, call_id: String, args: String },
    #[allow(dead_code)]
    ToolExecutionUpdate { call_id: String, update: ToolUpdateContent },
    ToolExecutionEnd { call_id: String, result: ToolResult },
    ThinkingDelta { content: String },
    Error { error: String },
}

/// Content inside a MessageUpdate event.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MessageUpdateContent {
    TextDelta { text: String },
    ToolCallStart { index: usize, id: String, name: String },
    ToolCallDelta { index: usize, arguments: String },
}

/// Content inside a ToolExecutionUpdate event.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolUpdateContent {
    #[allow(dead_code)]
    Progress { message: String },
    #[allow(dead_code)]
    PartialOutput { text: String },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::core::request::StopReason;

    #[test]
    fn agent_event_serializes_with_tag() {
        let event = AgentEvent::AgentStart;
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""type":"agent_start""#));
    }

    #[test]
    fn tool_execution_event_serializes() {
        let event = AgentEvent::ToolExecutionStart {
            tool_name: "file_read".into(),
            call_id: "call_123".into(),
            args: r#"{"path":"/tmp/test"}"#.into(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("file_read"));
        assert!(json.contains("call_123"));
        assert!(json.contains("args"));
    }

    #[test]
    fn message_update_serializes() {
        let event = AgentEvent::MessageUpdate {
            content: MessageUpdateContent::TextDelta { text: "hello".into() },
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("text_delta"));
        assert!(json.contains("hello"));
    }

    #[test]
    fn agent_end_serializes() {
        let event = AgentEvent::AgentEnd {
            result: AgentResult {
                text: "done".into(),
                actions: vec![],
            },
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""type":"agent_end""#));
        assert!(json.contains(r#""text":"done""#));
    }

    #[test]
    fn turn_start_serializes() {
        let event = AgentEvent::TurnStart { turn: 1 };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""type":"turn_start""#));
        assert!(json.contains(r#""turn":1"#));
    }

    #[test]
    fn message_end_serializes() {
        let event = AgentEvent::MessageEnd {
            stop_reason: StopReason::Stop,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""type":"message_end""#));
        assert!(json.contains(r#""stop_reason":"stop""#));
    }

    #[test]
    fn tool_execution_end_serializes() {
        let event = AgentEvent::ToolExecutionEnd {
            call_id: "call_1".into(),
            result: ToolResult {
                call_id: "call_1".into(),
                tool_name: "bash".into(),
                content: "output".into(),
                is_error: false,
            },
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""type":"tool_execution_end""#));
        assert!(json.contains("bash"));
    }

    #[test]
    fn thinking_delta_serializes() {
        let event = AgentEvent::ThinkingDelta {
            content: "thinking...".into(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""type":"thinking_delta""#));
        assert!(json.contains("thinking..."));
    }

    #[test]
    fn error_event_serializes() {
        let event = AgentEvent::Error {
            error: "something went wrong".into(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""type":"error""#));
        assert!(json.contains("something went wrong"));
    }

    #[test]
    fn tool_call_start_update_serializes() {
        let content = MessageUpdateContent::ToolCallStart {
            index: 0,
            id: "call_42".into(),
            name: "execute_command".into(),
        };
        let json = serde_json::to_string(&content).unwrap();
        assert!(json.contains(r#""kind":"tool_call_start""#));
        assert!(json.contains("execute_command"));
        assert!(json.contains("call_42"));
    }

    #[test]
    fn tool_call_delta_update_serializes() {
        let content = MessageUpdateContent::ToolCallDelta {
            index: 0,
            arguments: r#"{"command":"ls"}"#.into(),
        };
        let json = serde_json::to_string(&content).unwrap();
        assert!(json.contains(r#""kind":"tool_call_delta""#));
        assert!(json.contains(r#""arguments":"{\"command\":\"ls\"}""#));
    }

    #[test]
    fn tool_update_progress_serializes() {
        let update = ToolUpdateContent::Progress { message: "50%".into() };
        let json = serde_json::to_string(&update).unwrap();
        assert!(json.contains(r#""kind":"progress""#));
        assert!(json.contains("50%"));
    }

    #[test]
    fn tool_update_partial_output_serializes() {
        let update = ToolUpdateContent::PartialOutput { text: "partial...".into() };
        let json = serde_json::to_string(&update).unwrap();
        assert!(json.contains(r#""kind":"partial_output""#));
        assert!(json.contains("partial..."));
    }
}
