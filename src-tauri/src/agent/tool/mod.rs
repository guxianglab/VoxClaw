pub mod builtin;
pub mod hooks;

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use std::collections::HashMap;

use crate::agent::core::event::ToolUpdateContent;
use crate::agent::core::message::{ToolCall, ToolResult};
use crate::agent::error::AgentError;
use hooks::ToolHook;

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

/// Execution mode for multiple tool calls.
#[derive(Debug, Clone, Default)]
pub enum ExecutionMode {
    Parallel,
    #[default]
    Sequential,
}

/// Executes tool calls through the hook chain.
pub struct ToolExecutor {
    tools: HashMap<String, Box<dyn Tool>>,
    hooks: Vec<Box<dyn ToolHook>>,
    mode: ExecutionMode,
}

/// Result from running before-call hooks.
enum HookResult {
    Allowed { modified_args: Option<Value> },
    Blocked { reason: String },
}

impl ToolExecutor {
    pub fn new() -> Self {
        Self { tools: HashMap::new(), hooks: Vec::new(), mode: ExecutionMode::default() }
    }

    pub fn register(&mut self, tool: Box<dyn Tool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    pub fn add_hook(&mut self, hook: Box<dyn ToolHook>) {
        self.hooks.push(hook);
    }

    pub fn set_mode(&mut self, mode: ExecutionMode) {
        self.mode = mode;
    }

    pub fn tool_specs(&self) -> Vec<crate::agent::core::request::LlmTool> {
        self.tools.values().map(|t| {
            crate::agent::core::request::LlmTool::new(t.name(), t.description(), t.parameters())
        }).collect()
    }

    /// Execute a list of tool calls through hooks, return results.
    pub async fn execute_calls(&self, calls: Vec<ToolCall>) -> Vec<ToolResult> {
        let mut results = Vec::with_capacity(calls.len());

        for call in &calls {
            let tool_ref = self.tools.get(&call.name);

            // Run before hooks
            let hook_result = if let Some(tool) = tool_ref {
                self.run_before_hooks(call, tool.as_ref()).await
            } else {
                HookResult::Allowed { modified_args: None }
            };

            match hook_result {
                HookResult::Blocked { reason } => {
                    results.push(ToolResult {
                        call_id: call.id.clone(),
                        tool_name: call.name.clone(),
                        content: reason,
                        is_error: true,
                    });
                    continue;
                }
                HookResult::Allowed { modified_args } => {
                    let args = modified_args.unwrap_or_else(|| {
                        serde_json::from_str(&call.arguments).unwrap_or(json!({}))
                    });

                    let result = if let Some(tool) = tool_ref {
                        let (update_tx, _update_rx) = mpsc::channel::<ToolUpdateContent>(8);
                        let ctx = ToolContext {
                            _call_id: call.id.clone(),
                            _abort: CancellationToken::new(),
                            _on_update: update_tx,
                        };
                        match tool.execute(args, &ctx).await {
                            Ok(ToolOutput::Text(text)) => ToolResult { call_id: call.id.clone(), tool_name: call.name.clone(), content: text, is_error: false },
                            Ok(ToolOutput::Error(msg)) => ToolResult { call_id: call.id.clone(), tool_name: call.name.clone(), content: msg, is_error: true },
                            Err(e) => ToolResult { call_id: call.id.clone(), tool_name: call.name.clone(), content: e.to_string(), is_error: true },
                        }
                    } else {
                        ToolResult { call_id: call.id.clone(), tool_name: call.name.clone(), content: format!("Unknown tool: {}", call.name), is_error: true }
                    };

                    let mut final_result = result;
                    self.run_after_hooks(call, &mut final_result).await;
                    results.push(final_result);
                }
            }
        }

        results
    }

    async fn run_before_hooks(&self, call: &ToolCall, tool: &dyn Tool) -> HookResult {
        for hook in &self.hooks {
            match hook.before_call(call, tool).await {
                HookDecision::Allow => continue,
                HookDecision::Block { reason } => return HookResult::Blocked { reason },
                HookDecision::ModifyArgs { args } => return HookResult::Allowed { modified_args: Some(args) },
            }
        }
        HookResult::Allowed { modified_args: None }
    }

    async fn run_after_hooks(&self, call: &ToolCall, result: &mut ToolResult) {
        for hook in &self.hooks {
            let _ = hook.after_call(call, result).await;
        }
    }
}

/// Decision from a before-call hook.
#[derive(Debug, Clone)]
pub enum HookDecision {
    Allow,
    #[allow(dead_code)]
    Block { reason: String },
    #[allow(dead_code)]
    ModifyArgs { args: Value },
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
}
