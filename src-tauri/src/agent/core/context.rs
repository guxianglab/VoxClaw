use crate::agent::core::message::{AgentMessage, ToolResultContent, UserContent};
use crate::agent::core::request::{LlmFunctionCall, LlmMessage, LlmToolCall};
use crate::agent::error::AgentError;
use crate::agent::provider::LlmProvider;

/// Transforms agent messages before sending to LLM.
pub trait ContextTransformer: Send + Sync {
    fn transform(&self, messages: &mut Vec<AgentMessage>) -> Result<(), AgentError>;
}

/// Converts agent messages to provider-specific LLM messages.
pub trait ContextConverter: Send + Sync {
    fn convert(&self, messages: &[AgentMessage], provider: &dyn LlmProvider) -> Vec<LlmMessage>;
}

/// Default converter: simple mapping from AgentMessage to LlmMessage (OpenAI format).
pub struct DefaultConverter;

impl ContextConverter for DefaultConverter {
    fn convert(&self, messages: &[AgentMessage], _provider: &dyn LlmProvider) -> Vec<LlmMessage> {
        let mut result = Vec::with_capacity(messages.len());
        for msg in messages {
            match msg {
                AgentMessage::System { content } => {
                    result.push(LlmMessage::system(content));
                }
                AgentMessage::User { content, .. } => {
                    let UserContent::Text(text) = content;
                    result.push(LlmMessage::user(text));
                }
                AgentMessage::Assistant { content, tool_calls, .. } => {
                    if !tool_calls.is_empty() {
                        let calls: Vec<LlmToolCall> = tool_calls
                            .iter()
                            .map(|tc| LlmToolCall {
                                id: tc.id.clone(),
                                call_type: "function".into(),
                                function: LlmFunctionCall {
                                    name: tc.name.clone(),
                                    arguments: tc.arguments.clone(),
                                },
                            })
                            .collect();
                        result.push(LlmMessage::assistant_tool_calls(calls));
                    } else if let Some(text) = content {
                        result.push(LlmMessage::assistant_text(text));
                    }
                }
                AgentMessage::ToolResult {
                    tool_call_id,
                    content,
                    ..
                } => {
                    let ToolResultContent::Text(text) = content;
                    result.push(LlmMessage::tool_result(tool_call_id, text));
                }
            }
        }
        result
    }
}

/// Keep last N messages (always preserving system prompt).
pub struct TruncationTransformer {
    pub max_messages: usize,
}

impl ContextTransformer for TruncationTransformer {
    fn transform(&self, messages: &mut Vec<AgentMessage>) -> Result<(), AgentError> {
        if messages.len() <= self.max_messages {
            return Ok(());
        }
        let system_count = messages
            .iter()
            .take_while(|m| matches!(m, AgentMessage::System { .. }))
            .count();
        let non_system = &messages[system_count..];
        let keep = self.max_messages.saturating_sub(system_count);
        let start = non_system.len().saturating_sub(keep);
        let truncated: Vec<AgentMessage> = messages[..system_count]
            .iter()
            .chain(non_system[start..].iter())
            .cloned()
            .collect();
        *messages = truncated;
        Ok(())
    }
}

/// Trim messages based on estimated token count (~4 chars per token).
pub struct TokenBudgetTransformer {
    pub max_tokens: usize,
}

impl ContextTransformer for TokenBudgetTransformer {
    fn transform(&self, messages: &mut Vec<AgentMessage>) -> Result<(), AgentError> {
        let max_chars = self.max_tokens * 4;
        let total_chars: usize = messages.iter().map(estimate_chars).sum::<usize>();

        if total_chars <= max_chars {
            return Ok(());
        }

        let system_count = messages.iter().take_while(|m| matches!(m, AgentMessage::System { .. })).count();
        let non_system = &messages[system_count..];
        let mut budget = max_chars - messages[..system_count].iter().map(estimate_chars).sum::<usize>();
        let mut keep_from = 0usize;

        for (i, msg) in non_system.iter().enumerate().rev() {
            let msg_chars = estimate_chars(msg);
            if msg_chars <= budget {
                budget -= msg_chars;
                keep_from = i;
            } else {
                break;
            }
        }

        let truncated: Vec<AgentMessage> = messages[..system_count]
            .iter()
            .chain(non_system[keep_from..].iter())
            .cloned()
            .collect();
        *messages = truncated;
        Ok(())
    }
}

fn estimate_chars(msg: &AgentMessage) -> usize {
    match msg {
        AgentMessage::System { content } => content.len(),
        AgentMessage::User { content, .. } => match content { UserContent::Text(t) => t.len(), },
        AgentMessage::Assistant { content, tool_calls, .. } => {
            content.as_ref().map(|c| c.len()).unwrap_or(0)
                + tool_calls.iter().map(|tc| tc.arguments.len()).sum::<usize>()
        }
        AgentMessage::ToolResult { content, .. } => match content { ToolResultContent::Text(t) => t.len(), },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncation_transformer_keeps_recent() {
        let mut messages = vec![
            AgentMessage::system("sys"),
            AgentMessage::user("old 1"),
            AgentMessage::assistant_text("old reply 1"),
            AgentMessage::user("old 2"),
            AgentMessage::assistant_text("old reply 2"),
            AgentMessage::user("recent"),
        ];
        let transformer = TruncationTransformer { max_messages: 3 };
        transformer.transform(&mut messages).unwrap();
        assert_eq!(messages.len(), 3);
        assert!(matches!(&messages[0], AgentMessage::System { .. }));
        assert!(matches!(&messages[2], AgentMessage::User { .. }));
    }

    #[test]
    fn truncation_transformer_allows_all_when_under_limit() {
        let mut messages = vec![AgentMessage::user("hello")];
        let transformer = TruncationTransformer { max_messages: 10 };
        transformer.transform(&mut messages).unwrap();
        assert_eq!(messages.len(), 1);
    }

    // TODO: test DefaultConverter after provider trait is implemented

    #[test]
    fn token_budget_transformer_trims_long_conversation() {
        let mut messages = vec![
            AgentMessage::system("sys"),
            AgentMessage::user("a".repeat(500)),
            AgentMessage::assistant_text("b".repeat(500)),
            AgentMessage::user("c".repeat(500)),
            AgentMessage::assistant_text("d".repeat(500)),
            AgentMessage::user("e".repeat(500)),
        ];
        let transformer = TokenBudgetTransformer { max_tokens: 300 };
        transformer.transform(&mut messages).unwrap();
        assert!(messages.len() <= 4);
    }
}
