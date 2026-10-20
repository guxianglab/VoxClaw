pub mod builtin;

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use std::collections::HashMap;

use crate::agent::core::event::ToolUpdateContent;
use crate::agent::core::message::{ToolCall, ToolResult};
use crate::agent::error::AgentError;

/// Context passed to tool execution.
pub struct ToolContext {
    pub _call_id: String,
    pub _abort: CancellationToken,
    pub _on_update: mpsc::Sender<ToolUpdateContent>,
}

/// Output from a tool execution.
#[derive(Debug, Clone)]
pub enum ToolOutput {
    Text(String),
    Error(String),
}

/// Trait that all agent tools must implement.
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters(&self) -> Value;
    async fn execute(&self, args: Value, context: &ToolContext) -> Result<ToolOutput, AgentError>;
}

/// Executes tool calls sequentially.
pub struct ToolExecutor {
    tools: HashMap<String, Box<dyn Tool>>,
}

impl ToolExecutor {
    pub fn new() -> Self {
        Self { tools: HashMap::new() }
    }

    pub fn register(&mut self, tool: Box<dyn Tool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    pub fn tool_specs(&self) -> Vec<crate::agent::core::request::LlmTool> {
        self.tools.values().map(|t| {
            crate::agent::core::request::LlmTool::new(t.name(), t.description(), t.parameters())
        }).collect()
    }

    /// Execute a list of tool calls, return results.
    pub async fn execute_calls(&self, calls: Vec<ToolCall>) -> Vec<ToolResult> {
        let mut results = Vec::with_capacity(calls.len());

        for call in &calls {
            let error = |content: String| ToolResult {
                call_id: call.id.clone(),
                tool_name: call.name.clone(),
                content,
                is_error: true,
            };

            let Some(tool) = self.tools.get(&call.name) else {
                results.push(error(format!("Unknown tool: {}", call.name)));
                continue;
            };

            // Reject invalid JSON arguments up front: the model receives the
            // parse error as a tool result and can retry with fixed arguments,
            // instead of silently executing with `{}`.
            let args = match serde_json::from_str::<Value>(&call.arguments) {
                Ok(args) => args,
                Err(e) => {
                    results.push(error(format!(
                        "arguments are not valid JSON ({e}); fix the arguments and call again"
                    )));
                    continue;
                }
            };

            println!("[TOOL] {} args={}", call.name, call.arguments);
            let (update_tx, _update_rx) = mpsc::channel::<ToolUpdateContent>(8);
            let ctx = ToolContext {
                _call_id: call.id.clone(),
                _abort: CancellationToken::new(),
                _on_update: update_tx,
            };
            let result = match tool.execute(args, &ctx).await {
                Ok(ToolOutput::Text(text)) => ToolResult { call_id: call.id.clone(), tool_name: call.name.clone(), content: text, is_error: false },
                Ok(ToolOutput::Error(msg)) => error(msg),
                Err(e) => error(e.to_string()),
            };
            results.push(result);
        }

        results
    }
}

/// Create all built-in tools.
pub fn create_all_tools() -> Vec<Box<dyn Tool>> {
    builtin::create_all_tools()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct EchoTool;
    #[async_trait]
    impl Tool for EchoTool {
        fn name(&self) -> &str { "echo" }
        fn description(&self) -> &str { "Echo input" }
        fn parameters(&self) -> serde_json::Value {
            json!({ "type": "object", "properties": { "text": { "type": "string" } }, "required": ["text"] })
        }
        async fn execute(&self, args: serde_json::Value, _ctx: &ToolContext) -> Result<ToolOutput, AgentError> {
            Ok(ToolOutput::Text(args["text"].as_str().unwrap_or("").to_string()))
        }
    }

    #[tokio::test]
    async fn executor_runs_single_tool() {
        let mut executor = ToolExecutor::new();
        executor.register(Box::new(EchoTool));
        let call = ToolCall { id: "call_1".into(), name: "echo".into(), arguments: r#"{"text":"hi"}"#.into() };
        let results = executor.execute_calls(vec![call.clone()]).await;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].content, "hi");
        assert!(!results[0].is_error);
    }

    #[tokio::test]
    async fn executor_returns_error_for_unknown_tool() {
        let executor = ToolExecutor::new();
        let call = ToolCall { id: "call_1".into(), name: "nonexistent".into(), arguments: "{}".into() };
        let results = executor.execute_calls(vec![call]).await;
        assert_eq!(results.len(), 1);
        assert!(results[0].is_error);
    }

    #[tokio::test]
    async fn executor_rejects_invalid_json_arguments() {
        let mut executor = ToolExecutor::new();
        executor.register(Box::new(EchoTool));
        let call = ToolCall {
            id: "call_bad".into(),
            name: "echo".into(),
            // Malformed: unquoted key — must not fall back to `{}` and run anyway.
            arguments: r#"{text: "hi"}"#.into(),
        };
        let results = executor.execute_calls(vec![call]).await;
        assert_eq!(results.len(), 1);
        assert!(results[0].is_error);
        assert!(results[0].content.contains("not valid JSON"));
    }
}
