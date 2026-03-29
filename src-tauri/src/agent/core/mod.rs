pub mod context;
pub mod event;
pub mod message;
pub mod request;

use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use futures_util::StreamExt;

use crate::agent::provider::LlmProvider;
use crate::agent::tool::{Tool, ToolExecutor, ExecutionMode};
use crate::agent::tool::hooks::ToolHook;
use crate::agent::core::request::{LlmRequest, LlmStreamEvent, StopReason, ThinkingConfig, ThinkingLevel};
use crate::agent::core::event::{AgentEvent, MessageUpdateContent};
use crate::agent::core::message::{AgentMessage, ToolCall, AgentResult, ActionSummary, default_system_prompt};
use crate::agent::core::context::{ContextTransformer, ContextConverter, DefaultConverter};
use crate::agent::error::AgentError;

/// The AI agent.
pub struct Agent {
    system_prompt: String,
    model: String,
    provider: Box<dyn LlmProvider>,
    thinking: ThinkingConfig,
    executor: ToolExecutor,
    messages: Vec<AgentMessage>,
    context_transformers: Vec<Box<dyn ContextTransformer>>,
    context_converter: Box<dyn ContextConverter>,
    max_iterations: u32,
    cancel_token: CancellationToken,
    event_tx: broadcast::Sender<AgentEvent>,
}

pub struct AgentBuilder {
    model: String,
    provider: Option<Box<dyn LlmProvider>>,
    tools: Vec<Box<dyn Tool>>,
    hooks: Vec<Box<dyn ToolHook>>,
    system_prompt: Option<String>,
    thinking: ThinkingConfig,
    context_transformers: Vec<Box<dyn ContextTransformer>>,
    context_converter: Box<dyn ContextConverter>,
    max_iterations: u32,
    execution_mode: ExecutionMode,
}

impl AgentBuilder {
    pub fn new() -> Self {
        Self {
            model: String::new(),
            provider: None,
            tools: Vec::new(),
            hooks: Vec::new(),
            thinking: ThinkingConfig { level: ThinkingLevel::None, budget_tokens: None },
            system_prompt: None,
            context_transformers: Vec::new(),
            context_converter: Box::new(DefaultConverter),
            max_iterations: 10,
            execution_mode: ExecutionMode::default(),
        }
    }

    pub fn model(mut self, model: impl Into<String>) -> Self { self.model = model.into(); self }
    pub fn provider(mut self, provider: Box<dyn LlmProvider>) -> Self { self.provider = Some(provider); self }
    pub fn tool(mut self, tool: Box<dyn Tool>) -> Self { self.tools.push(tool); self }
    pub fn hook(mut self, hook: Box<dyn ToolHook>) -> Self { self.hooks.push(hook); self }
    pub fn thinking(mut self, config: ThinkingConfig) -> Self { self.thinking = config; self }
    pub fn context_transformer(mut self, t: Box<dyn ContextTransformer>) -> Self { self.context_transformers.push(t); self }
    pub fn max_iterations(mut self, n: u32) -> Self { self.max_iterations = n; self }
    pub fn execution_mode(mut self, mode: ExecutionMode) -> Self { self.execution_mode = mode; self }
    #[allow(dead_code)]
    pub fn system_prompt(mut self, prompt: impl Into<String>) -> Self { self.system_prompt = Some(prompt.into()); self }

    pub fn build(self) -> Result<Agent, AgentError> {
        let provider = self.provider.ok_or(AgentError::Provider("Agent requires a provider".into()))?;
        let mut executor = ToolExecutor::new();
        for tool in self.tools {
            executor.register(tool);
        }
        for hook in self.hooks {
            executor.add_hook(hook);
        }
        executor.set_mode(self.execution_mode);
        let (event_tx, _) = broadcast::channel(64);
        Ok(Agent {
            system_prompt: self.system_prompt.unwrap_or_else(|| default_system_prompt().to_string()),
            model: self.model,
            provider,
            thinking: self.thinking,
            executor,
            messages: Vec::new(),
            context_transformers: self.context_transformers,
            context_converter: self.context_converter,
            max_iterations: self.max_iterations,
            cancel_token: CancellationToken::new(),
            event_tx,
        })
    }
}

impl Default for AgentBuilder {
    fn default() -> Self { Self::new() }
}

impl Agent {
    pub fn subscribe(&self) -> broadcast::Receiver<AgentEvent> {
        self.event_tx.subscribe()
    }

    /// Returns a clone of the agent's cancellation token.
    /// Call `.cancel()` on the returned token to abort an in-progress `process()` call.
    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancel_token.clone()
    }

    pub fn reset(&mut self) {
        self.messages.clear();
        self.cancel_token = CancellationToken::new();
    }

    pub async fn process(&mut self, user_text: &str) -> Result<AgentResult, AgentError> {
        self.reset();
        self.messages.push(AgentMessage::user(user_text));
        let _ = self.event_tx.send(AgentEvent::AgentStart);
        let mut final_text = String::new();
        let mut all_actions = Vec::new();

        for turn in 0..self.max_iterations {
            if self.cancel_token.is_cancelled() {
                return Err(AgentError::Cancelled);
            }
            let _ = self.event_tx.send(AgentEvent::TurnStart { turn });

            // Context pipeline
            let mut transformed_messages = self.messages.clone();
            for transformer in &self.context_transformers {
                transformer.transform(&mut transformed_messages)?;
            }

            // Convert to LLM messages
            let mut llm_messages = vec![crate::agent::core::request::LlmMessage::system(&self.system_prompt)];
            llm_messages.extend(self.context_converter.convert(&transformed_messages, self.provider.as_ref()));

            // Build request
            println!("[AGENT] Turn {}: sending {} messages, {} tools", turn, llm_messages.len(), self.executor.tool_specs().len());
            let request = LlmRequest {
                model: self.model.clone(),
                messages: llm_messages,
                tools: self.executor.tool_specs(),
                thinking: if self.thinking.level == ThinkingLevel::None { None } else { Some(self.thinking.clone()) },
                max_tokens: None,
                temperature: None,
            };

            // Stream LLM response
            let mut stream = self.provider.stream(request).await?;
            let _ = self.event_tx.send(AgentEvent::MessageStart);

            let mut text_buf = String::new();
            let mut tool_calls: Vec<ToolCall> = Vec::new();
            let mut stop_reason = StopReason::Stop;

            while let Some(result) = stream.next().await {
                if self.cancel_token.is_cancelled() {
                    return Err(AgentError::Cancelled);
                }
                let event = match result {
                    Ok(e) => e,
                    Err(e) => {
                        eprintln!("[AGENT] Stream error at turn {}: {}", turn, e);
                        let _ = self.event_tx.send(AgentEvent::Error { error: e.to_string() });
                        return Err(e);
                    }
                };
                match event {
                    LlmStreamEvent::TextDelta(delta) => {
                        text_buf.push_str(&delta);
                        let _ = self.event_tx.send(AgentEvent::MessageUpdate {
                            content: MessageUpdateContent::TextDelta { text: delta },
                        });
                    }
                    LlmStreamEvent::ThinkingDelta(content) => {
                        let _ = self.event_tx.send(AgentEvent::ThinkingDelta { content });
                    }
                    LlmStreamEvent::ToolCallStart { index, id, name } => {
                        tool_calls.push(ToolCall { id: id.clone(), name: name.clone(), arguments: String::new() });
                        let _ = self.event_tx.send(AgentEvent::MessageUpdate {
                            content: MessageUpdateContent::ToolCallStart { index, id, name },
                        });
                    }
                    LlmStreamEvent::ToolCallDelta { index, arguments_delta } => {
                        if let Some(tc) = tool_calls.get_mut(index) {
                            tc.arguments.push_str(&arguments_delta);
                        }
                        let _ = self.event_tx.send(AgentEvent::MessageUpdate {
                            content: MessageUpdateContent::ToolCallDelta { index, arguments: arguments_delta },
                        });
                    }
                    LlmStreamEvent::ToolCallEnd { .. } => {}
                    LlmStreamEvent::Done { stop_reason: sr } => {
                        stop_reason = sr;
                        break;
                    }
                }
            }

            println!("[AGENT] Turn {}: received {} text chars, {} tool calls, stop_reason={:?}", turn, text_buf.len(), tool_calls.len(), stop_reason);
            let _ = self.event_tx.send(AgentEvent::MessageEnd { stop_reason: stop_reason.clone() });

            // No tool calls -> return text
            if tool_calls.is_empty() {
                final_text = text_buf;
                break;
            }

            // Emit start events before execution
            for tc in &tool_calls {
                let _ = self.event_tx.send(AgentEvent::ToolExecutionStart {
                    tool_name: tc.name.clone(),
                    call_id: tc.id.clone(),
                    args: tc.arguments.clone(),
                });
            }

            // Execute tools
            println!("[AGENT] Turn {}: executing {} tool calls", turn, tool_calls.len());
            self.messages.push(AgentMessage::assistant_tool_calls(tool_calls.clone()));
            let results = self.executor.execute_calls(tool_calls).await;

            for result in &results {
                println!("[AGENT] Tool result: {} -> success={}, output_len={}", result.tool_name, !result.is_error, result.content.len());
                let preview = if result.content.chars().count() > 500 {
                    format!("{}...", result.content.chars().take(500).collect::<String>())
                } else {
                    result.content.clone()
                };
                all_actions.push(ActionSummary {
                    tool_name: result.tool_name.clone(),
                    success: !result.is_error,
                    output_preview: preview,
                });
                self.messages.push(AgentMessage::tool_result(&result.call_id, &result.content));
                let _ = self.event_tx.send(AgentEvent::ToolExecutionEnd {
                    call_id: result.call_id.clone(),
                    result: result.clone(),
                });
            }
            let _ = self.event_tx.send(AgentEvent::TurnEnd { turn });
        }

        let _ = self.event_tx.send(AgentEvent::AgentEnd {
            result: AgentResult { text: final_text.clone(), actions: all_actions.clone() },
        });
        Ok(AgentResult { text: final_text, actions: all_actions })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use crate::agent::provider::LlmProvider;
    use crate::agent::core::request::*;
    use crate::agent::error::AgentError;
    use crate::agent::tool::{Tool, ToolContext, ToolOutput};
    use futures_util::stream;
    use serde_json::json;

    struct MockProvider {
        response_events: Vec<LlmStreamEvent>,
    }

    #[async_trait]
    impl LlmProvider for MockProvider {
        async fn stream(&self, _req: LlmRequest) -> Result<LlmStream, AgentError> {
            Ok(Box::pin(stream::iter(self.response_events.clone().into_iter().map(Ok))))
        }
        fn name(&self) -> &str { "mock" }
        fn capabilities(&self) -> crate::agent::provider::ProviderCapabilities { Default::default() }
    }

    #[tokio::test]
    async fn agent_returns_text_response() {
        let provider = MockProvider {
            response_events: vec![
                LlmStreamEvent::TextDelta("Hello!".into()),
                LlmStreamEvent::Done { stop_reason: StopReason::Stop },
            ],
        };
        let mut agent = AgentBuilder::new().provider(Box::new(provider)).model("test").build().unwrap();
        let result = agent.process("Say hello").await.unwrap();
        assert_eq!(result.text, "Hello!");
    }

    #[tokio::test]
    async fn agent_tool_call_loop() {
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

        struct MultiMockProvider {
            responses: Vec<Vec<LlmStreamEvent>>,
            call_index: std::sync::atomic::AtomicUsize,
        }
        impl MultiMockProvider {
            fn new(responses: Vec<Vec<LlmStreamEvent>>) -> Self {
                Self { responses, call_index: std::sync::atomic::AtomicUsize::new(0) }
            }
        }
        #[async_trait]
        impl LlmProvider for MultiMockProvider {
            async fn stream(&self, _req: LlmRequest) -> Result<LlmStream, AgentError> {
                let idx = self.call_index.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let events = self.responses.get(idx).cloned().unwrap_or_default();
                Ok(Box::pin(stream::iter(events.into_iter().map(Ok))))
            }
            fn name(&self) -> &str { "multi_mock" }
            fn capabilities(&self) -> crate::agent::provider::ProviderCapabilities { Default::default() }
        }

        let provider = MultiMockProvider::new(vec![
            vec![
                LlmStreamEvent::ToolCallStart { index: 0, id: "call_1".into(), name: "echo".into() },
                LlmStreamEvent::ToolCallDelta { index: 0, arguments_delta: r#"{"text":"ping"}"#.into() },
                LlmStreamEvent::ToolCallEnd { _index: 0 },
                LlmStreamEvent::Done { stop_reason: StopReason::ToolCalls },
            ],
            vec![
                LlmStreamEvent::TextDelta("Echo: ping".into()),
                LlmStreamEvent::Done { stop_reason: StopReason::Stop },
            ],
        ]);

        let mut builder = AgentBuilder::new().provider(Box::new(provider)).model("test");
        builder = builder.tool(Box::new(EchoTool));
        let mut agent = builder.build().unwrap();
        let result = agent.process("Test").await.unwrap();
        assert_eq!(result.text, "Echo: ping");
        assert_eq!(result.actions.len(), 1);
        assert!(result.actions[0].success);
    }

    #[tokio::test]
    async fn agent_emits_events() {
        let provider = MockProvider {
            response_events: vec![
                LlmStreamEvent::TextDelta("Hi".into()),
                LlmStreamEvent::Done { stop_reason: StopReason::Stop },
            ],
        };
        let mut agent = AgentBuilder::new().provider(Box::new(provider)).model("test").build().unwrap();
        let mut rx = agent.subscribe();
        let _ = agent.process("Hello").await;
        // Should have received AgentStart, TurnStart, MessageStart, TextDelta, MessageEnd, AgentEnd
        let mut events = Vec::new();
        while let Ok(event) = rx.try_recv() {
            events.push(event);
        }
        assert!(events.len() >= 4);
        assert!(matches!(&events[0], AgentEvent::AgentStart));
        assert!(matches!(&events[1], AgentEvent::TurnStart { .. }));
        assert!(matches!(&events[2], AgentEvent::MessageStart));
    }

    #[tokio::test]
    async fn agent_builder_default_system_prompt() {
        let provider = MockProvider {
            response_events: vec![
                LlmStreamEvent::TextDelta("OK".into()),
                LlmStreamEvent::Done { stop_reason: StopReason::Stop },
            ],
        };
        let mut agent = AgentBuilder::new()
            .provider(Box::new(provider))
            .model("test")
            .build()
            .unwrap();
        // System prompt should contain "VoxClaw Agent"
        let _ = agent.process("hi").await;
    }

    #[tokio::test]
    async fn agent_builder_custom_system_prompt() {
        let provider = MockProvider {
            response_events: vec![
                LlmStreamEvent::TextDelta("OK".into()),
                LlmStreamEvent::Done { stop_reason: StopReason::Stop },
            ],
        };
        let mut agent = AgentBuilder::new()
            .provider(Box::new(provider))
            .model("test")
            .system_prompt("You are a custom assistant.")
            .build()
            .unwrap();
        let _ = agent.process("hi").await;
    }

    #[test]
    fn agent_builder_requires_provider() {
        let result = AgentBuilder::new().model("test").build();
        assert!(result.is_err());
    }
}
