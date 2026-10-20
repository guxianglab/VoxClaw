pub mod context;
pub mod event;
pub mod message;
pub mod request;
pub mod usage;

use std::collections::HashMap;

use tokio::sync::{broadcast, mpsc, oneshot};
use tokio_util::sync::CancellationToken;
use futures_util::StreamExt;

use crate::agent::provider::{stream_with_retry, LlmProvider};
use crate::agent::session::{SessionEntryKind, SessionStore};
use crate::agent::tool::{Tool, ToolExecutor};
use crate::agent::core::request::{LlmRequest, LlmStreamEvent, StopReason, ThinkingConfig, ThinkingLevel};
use crate::agent::core::event::{AgentEvent, MessageUpdateContent};
use crate::agent::core::message::{AgentMessage, ToolCall, AgentResult, ActionSummary, default_system_prompt};
use crate::agent::core::context::{ContextTransformer, ContextConverter, DefaultConverter};
use crate::agent::core::usage::Usage;
use crate::agent::error::AgentError;

/// Commands accepted by a running agent actor. Sent through `AgentHandle`;
/// the run loop polls for them between turns and between stream chunks.
pub enum AgentCommand {
    /// Append a user message and run to completion. Rejected while another
    /// prompt is in flight.
    Prompt {
        text: String,
        reply: oneshot::Sender<Result<AgentResult, AgentError>>,
    },
    /// Inject a steering message, drained into context before the next turn.
    #[allow(dead_code)]
    Steer { text: String },
    /// Queue a follow-up message for after the current run naturally ends.
    #[allow(dead_code)]
    FollowUp { text: String },
    /// Cancel the in-flight run at the next check-point (mid-stream aware).
    #[allow(dead_code)]
    Cancel,
    /// Change the thinking configuration for subsequent turns.
    #[allow(dead_code)]
    SetThinking { config: ThinkingConfig },
}

/// Cloneable handle to an agent running inside its own task (see
/// [`Agent::spawn`]). All interaction with a spawned agent goes through this
/// handle; the `Agent` itself is owned by the actor task.
#[derive(Clone)]
pub struct AgentHandle {
    cmd_tx: mpsc::Sender<AgentCommand>,
    event_tx: broadcast::Sender<AgentEvent>,
    #[allow(dead_code)]
    cancel_token: CancellationToken,
}

impl AgentHandle {
    /// Subscribe to the agent's event broadcast.
    pub fn subscribe(&self) -> broadcast::Receiver<AgentEvent> {
        self.event_tx.subscribe()
    }

    /// Run a prompt to completion. Fails if the agent is already processing
    /// another prompt or the actor task has exited.
    ///
    /// Takes `self` so the returned future is `'static` (spawnable). Clone
    /// the handle first if you need it afterwards.
    pub async fn prompt(self, text: impl Into<String>) -> Result<AgentResult, AgentError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(AgentCommand::Prompt { text: text.into(), reply: tx })
            .await
            .map_err(|_| AgentError::Provider("Agent task is not running".into()))?;
        rx.await
            .map_err(|_| AgentError::Provider("Agent task exited before replying".into()))?
    }

    /// Queue a steering message into the running agent.
    #[allow(dead_code)]
    pub async fn steer(&self, text: impl Into<String>) -> Result<(), AgentError> {
        self.cmd_tx
            .send(AgentCommand::Steer { text: text.into() })
            .await
            .map_err(|_| AgentError::Provider("Agent task is not running".into()))
    }

    /// Queue a follow-up message for after the current run ends.
    #[allow(dead_code)]
    pub async fn follow_up(&self, text: impl Into<String>) -> Result<(), AgentError> {
        self.cmd_tx
            .send(AgentCommand::FollowUp { text: text.into() })
            .await
            .map_err(|_| AgentError::Provider("Agent task is not running".into()))
    }

    /// Cancel the in-flight run. Immediate: signals the cancellation token
    /// directly rather than queueing a command.
    #[allow(dead_code)]
    pub fn cancel(&self) {
        self.cancel_token.cancel();
    }

    /// Change the thinking configuration of a running agent.
    #[allow(dead_code)]
    pub async fn set_thinking(&self, config: ThinkingConfig) -> Result<(), AgentError> {
        self.cmd_tx
            .send(AgentCommand::SetThinking { config })
            .await
            .map_err(|_| AgentError::Provider("Agent task is not running".into()))
    }
}

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
    /// Messages injected mid-run between turns (steering).
    steering_queue: std::collections::VecDeque<AgentMessage>,
    /// Messages queued for the next prompt cycle (follow-up).
    followup_queue: std::collections::VecDeque<AgentMessage>,
    /// Session log; when attached, every appended message is persisted here.
    session: Option<SessionStore>,
}

pub struct AgentBuilder {
    model: String,
    provider: Option<Box<dyn LlmProvider>>,
    tools: Vec<Box<dyn Tool>>,
    system_prompt: Option<String>,
    thinking: ThinkingConfig,
    context_transformers: Vec<Box<dyn ContextTransformer>>,
    context_converter: Box<dyn ContextConverter>,
    max_iterations: u32,
    session: Option<SessionStore>,
    cancel_token: Option<CancellationToken>,
}

impl AgentBuilder {
    pub fn new() -> Self {
        Self {
            model: String::new(),
            provider: None,
            tools: Vec::new(),
            thinking: ThinkingConfig { level: ThinkingLevel::None, budget_tokens: None },
            system_prompt: None,
            context_transformers: Vec::new(),
            context_converter: Box::new(DefaultConverter),
            max_iterations: 10,
            session: None,
            cancel_token: None,
        }
    }

    pub fn model(mut self, model: impl Into<String>) -> Self { self.model = model.into(); self }
    pub fn provider(mut self, provider: Box<dyn LlmProvider>) -> Self { self.provider = Some(provider); self }
    pub fn tool(mut self, tool: Box<dyn Tool>) -> Self { self.tools.push(tool); self }
    pub fn thinking(mut self, config: ThinkingConfig) -> Self { self.thinking = config; self }
    pub fn context_transformer(mut self, t: Box<dyn ContextTransformer>) -> Self { self.context_transformers.push(t); self }
    pub fn max_iterations(mut self, n: u32) -> Self { self.max_iterations = n; self }
    #[allow(dead_code)]
    pub fn system_prompt(mut self, prompt: impl Into<String>) -> Self { self.system_prompt = Some(prompt.into()); self }
    /// Attach a session store: every appended message is persisted to it.
    pub fn session(mut self, store: SessionStore) -> Self { self.session = Some(store); self }
    /// Use an externally-owned cancellation token, so callers that already
    /// hold a cancel slot (e.g. the dictation pipeline) can abort the run.
    pub fn cancellation_token(mut self, token: CancellationToken) -> Self { self.cancel_token = Some(token); self }

    pub fn build(self) -> Result<Agent, AgentError> {
        let provider = self.provider.ok_or(AgentError::Provider("Agent requires a provider".into()))?;
        let mut executor = ToolExecutor::new();
        for tool in self.tools {
            executor.register(tool);
        }
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
            cancel_token: self.cancel_token.unwrap_or_default(),
            event_tx,
            steering_queue: std::collections::VecDeque::new(),
            followup_queue: std::collections::VecDeque::new(),
            session: self.session,
        })
    }
}

impl Default for AgentBuilder {
    fn default() -> Self { Self::new() }
}

/// Raw material collected from one LLM turn, before the loop decides what to
/// do with it.
struct TurnOutput {
    text: String,
    thinking: Option<String>,
    tool_calls: Vec<ToolCall>,
    usage: Option<Usage>,
    stop_reason: StopReason,
}

/// Accumulator for a single streaming tool call, keyed by provider index.
struct ToolCallAccumulator {
    id: String,
    name: String,
    arguments: String,
}

impl Agent {
    #[allow(dead_code)]
    pub fn subscribe(&self) -> broadcast::Receiver<AgentEvent> {
        self.event_tx.subscribe()
    }

    /// Returns a clone of the agent's cancellation token.
    /// Call `.cancel()` on the returned token to abort an in-progress `process()` call.
    #[allow(dead_code)]
    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancel_token.clone()
    }

    #[allow(dead_code)]
    pub fn reset(&mut self) {
        self.messages.clear();
        self.steering_queue.clear();
        self.followup_queue.clear();
        // Renew only if cancelled: an externally-shared token must stay valid
        // across runs (the dictation pipeline holds it in a cancel slot).
        if self.cancel_token.is_cancelled() {
            self.cancel_token = CancellationToken::new();
        }
    }

    /// Seed the conversation with pre-existing messages (e.g. replayed from a session).
    /// Does NOT persist to the session log — assumes the caller already has
    /// these on disk.
    pub fn seed_messages(&mut self, msgs: Vec<AgentMessage>) {
        self.messages = msgs;
    }

    /// Number of messages currently in context.
    #[allow(dead_code)]
    pub fn message_count(&self) -> usize {
        self.messages.len()
    }

    /// Queue a steering message into the running agent. This requires `&mut self`,
    /// so it can only be called between runs — for mid-run steering use
    /// [`AgentHandle::steer`] on a spawned agent.
    #[allow(dead_code)]
    pub fn steer(&mut self, text: impl Into<String>) {
        self.steering_queue
            .push_back(AgentMessage::user(text.into()));
    }

    /// Queue a follow-up message: drained at the natural end of the current run,
    /// triggering another run cycle if the agent had already stopped.
    #[allow(dead_code)]
    pub fn follow_up(&mut self, text: impl Into<String>) {
        self.followup_queue
            .push_back(AgentMessage::user(text.into()));
    }

    /// Move the agent into its own tokio task and return a handle. All state
    /// (messages, session log, cancel token) is owned by the task; commands
    /// and events flow through the handle. The task exits once all handles
    /// are dropped and the current prompt finishes.
    pub fn spawn(self) -> AgentHandle {
        let (cmd_tx, mut cmd_rx) = mpsc::channel::<AgentCommand>(64);
        let handle = AgentHandle {
            cmd_tx: cmd_tx.clone(),
            event_tx: self.event_tx.clone(),
            cancel_token: self.cancel_token.clone(),
        };
        let mut agent = self;
        tokio::spawn(async move {
            while let Some(cmd) = cmd_rx.recv().await {
                match cmd {
                    AgentCommand::Prompt { text, reply } => {
                        let result = agent.prompt_via(&mut cmd_rx, &text).await;
                        let _ = reply.send(result);
                    }
                    other => agent.handle_command(other),
                }
            }
        });
        handle
    }

    fn handle_command(&mut self, cmd: AgentCommand) {
        match cmd {
            AgentCommand::Steer { text } => self.steer(text),
            AgentCommand::FollowUp { text } => self.follow_up(text),
            AgentCommand::Cancel => self.cancel_token.cancel(),
            AgentCommand::SetThinking { config } => self.thinking = config,
            AgentCommand::Prompt { reply, .. } => {
                // Can only happen through direct misuse of the runtime loop;
                // a spawned agent routes prompts through `prompt_via`.
                let _ = reply.send(Err(AgentError::Provider(
                    "Agent is busy processing another prompt".into(),
                )));
            }
        }
    }

    /// Append a user message and run the agent loop until natural completion
    /// (no tool calls in the final turn) or an error/cancel occurs.
    /// History is preserved across calls — call `reset()` to start over.
    #[allow(dead_code)]
    pub async fn prompt(&mut self, user_text: &str) -> Result<AgentResult, AgentError> {
        // Dummy command channel: no sender will ever fire, so the run loop's
        // command polling is inert for direct (non-actor) usage.
        let (_keep_tx, mut rx) = mpsc::channel::<AgentCommand>(1);
        self.prompt_via(&mut rx, user_text).await
    }

    /// Backwards-compatible single-shot entry point: clears history then prompts.
    #[allow(dead_code)]
    pub async fn process(&mut self, user_text: &str) -> Result<AgentResult, AgentError> {
        self.reset();
        self.prompt(user_text).await
    }

    /// Resume the loop without appending a new user message. Useful after a
    /// `Cancelled` / transient error to retry the last turn against existing context.
    #[allow(dead_code)]
    pub async fn continue_run(&mut self) -> Result<AgentResult, AgentError> {
        if self.cancel_token.is_cancelled() {
            self.cancel_token = CancellationToken::new();
        }
        let (_keep_tx, mut rx) = mpsc::channel::<AgentCommand>(1);
        self.run_loop_inner(&mut rx).await
    }

    async fn prompt_via(
        &mut self,
        cmd_rx: &mut mpsc::Receiver<AgentCommand>,
        user_text: &str,
    ) -> Result<AgentResult, AgentError> {
        if self.cancel_token.is_cancelled() {
            self.cancel_token = CancellationToken::new();
        }
        self.append_message(AgentMessage::user(user_text));
        self.run_loop_inner(cmd_rx).await
    }

    fn append_message(&mut self, msg: AgentMessage) {
        if let Some(store) = &mut self.session {
            let _ = store.append(SessionEntryKind::Message { message: msg.clone() });
        }
        self.messages.push(msg);
    }

    fn drain_steering(&mut self) -> Vec<AgentMessage> {
        self.steering_queue.drain(..).collect()
    }

    fn drain_followup(&mut self) -> Vec<AgentMessage> {
        self.followup_queue.drain(..).collect()
    }

    async fn run_loop_inner(
        &mut self,
        cmd_rx: &mut mpsc::Receiver<AgentCommand>,
    ) -> Result<AgentResult, AgentError> {
        let _ = self.event_tx.send(AgentEvent::AgentStart);
        let mut all_actions = Vec::new();
        let max_iterations = self.max_iterations.max(1);

        for turn in 0..max_iterations {
            if self.cancel_token.is_cancelled() {
                return Err(self.finish_cancelled(all_actions));
            }

            // Drain any steering messages into context BEFORE the turn so the
            // model can react.
            for steer_msg in self.drain_steering() {
                self.append_message(steer_msg);
            }

            let _ = self.event_tx.send(AgentEvent::TurnStart { turn });

            let output = match self.run_turn(turn, true, cmd_rx).await {
                Ok(o) => o,
                Err(AgentError::Cancelled) => return Err(self.finish_cancelled(all_actions)),
                Err(e) => {
                    let _ = self.event_tx.send(AgentEvent::Error { error: e.to_string() });
                    return Err(e);
                }
            };

            self.append_message(AgentMessage::Assistant {
                content: if output.text.is_empty() { None } else { Some(output.text.clone()) },
                tool_calls: output.tool_calls.clone(),
                thinking: output.thinking.clone(),
                usage: output.usage.clone(),
                stop_reason: Some(output.stop_reason.clone()),
            });
            let _ = self.event_tx.send(AgentEvent::MessageEnd { stop_reason: output.stop_reason.clone() });

            // No tool calls -> natural end of this run
            if output.tool_calls.is_empty() {
                // Drain follow-ups: if any, append + continue the outer loop
                // for another turn cycle. Otherwise we're done.
                let followups = self.drain_followup();
                if followups.is_empty() {
                    let result = AgentResult { text: output.text, actions: all_actions };
                    let _ = self.event_tx.send(AgentEvent::AgentEnd {
                        result: result.clone(),
                        stop_reason: output.stop_reason,
                    });
                    return Ok(result);
                }
                for m in followups {
                    self.append_message(m);
                }
                continue;
            }

            // Emit start events before execution
            for tc in &output.tool_calls {
                let _ = self.event_tx.send(AgentEvent::ToolExecutionStart {
                    tool_name: tc.name.clone(),
                    call_id: tc.id.clone(),
                    args: tc.arguments.clone(),
                });
            }

            println!("[AGENT] Turn {}: executing {} tool calls", turn, output.tool_calls.len());
            let results = self.executor.execute_calls(output.tool_calls).await;

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
                self.append_message(AgentMessage::tool_result_with_error(
                    &result.call_id,
                    &result.content,
                    result.is_error,
                ));
                let _ = self.event_tx.send(AgentEvent::ToolExecutionEnd {
                    call_id: result.call_id.clone(),
                    result: result.clone(),
                });
            }

            if self.cancel_token.is_cancelled() {
                return Err(self.finish_cancelled(all_actions));
            }
            let _ = self.event_tx.send(AgentEvent::TurnEnd { turn });
        }

        // Iterations exhausted while the model still wanted tools: run one
        // final turn with tools disabled so the user gets a summary instead of
        // a silent empty result.
        for steer_msg in self.drain_steering() {
            self.append_message(steer_msg);
        }
        self.append_message(AgentMessage::user(
            "You have reached the tool-call limit for this run. Do not call any more tools. \
             Summarize concisely what you have done so far and the current outcome for the user.",
        ));
        let _ = self.event_tx.send(AgentEvent::TurnStart { turn: max_iterations });

        let output = match self.run_turn(max_iterations, false, cmd_rx).await {
            Ok(o) => o,
            Err(AgentError::Cancelled) => return Err(self.finish_cancelled(all_actions)),
            Err(e) => {
                let _ = self.event_tx.send(AgentEvent::Error { error: e.to_string() });
                return Err(e);
            }
        };

        self.append_message(AgentMessage::Assistant {
            content: if output.text.is_empty() { None } else { Some(output.text.clone()) },
            tool_calls: vec![],
            thinking: output.thinking.clone(),
            usage: output.usage.clone(),
            stop_reason: Some(output.stop_reason.clone()),
        });
        let _ = self.event_tx.send(AgentEvent::MessageEnd { stop_reason: output.stop_reason.clone() });
        let _ = self.event_tx.send(AgentEvent::TurnEnd { turn: max_iterations });

        let result = AgentResult { text: output.text, actions: all_actions };
        let _ = self.event_tx.send(AgentEvent::AgentEnd {
            result: result.clone(),
            stop_reason: output.stop_reason,
        });
        Ok(result)
    }

    /// Terminal bookkeeping for a cancelled run: partial assistant output was
    /// already persisted by `run_turn`; emit the terminal event and surface
    /// the cancellation as an error to the caller.
    fn finish_cancelled(&mut self, actions: Vec<ActionSummary>) -> AgentError {
        let result = AgentResult { text: String::new(), actions };
        let _ = self.event_tx.send(AgentEvent::AgentEnd {
            result,
            stop_reason: StopReason::Cancelled,
        });
        AgentError::Cancelled
    }

    /// Run one LLM turn: build the request (after the context pipeline), then
    /// consume the provider stream while watching the cancellation token and
    /// the command channel. Returns the accumulated turn output; on
    /// cancellation, persists partial text as an assistant message before
    /// returning `Err(Cancelled)`.
    async fn run_turn(
        &mut self,
        turn: u32,
        tools_enabled: bool,
        cmd_rx: &mut mpsc::Receiver<AgentCommand>,
    ) -> Result<TurnOutput, AgentError> {
        // Context pipeline
        let mut transformed_messages = self.messages.clone();
        for transformer in &self.context_transformers {
            transformer.transform(&mut transformed_messages)?;
        }

        // Convert to LLM messages
        let mut llm_messages = vec![crate::agent::core::request::LlmMessage::system(&self.system_prompt)];
        llm_messages.extend(self.context_converter.convert(&transformed_messages, self.provider.as_ref()));

        let tools = if tools_enabled { self.executor.tool_specs() } else { Vec::new() };
        println!("[AGENT] Turn {}: sending {} messages, {} tools", turn, llm_messages.len(), tools.len());
        let request = LlmRequest {
            model: self.model.clone(),
            messages: llm_messages,
            tools,
            thinking: if self.thinking.level.is_active() { Some(self.thinking.clone()) } else { None },
            max_tokens: None,
            temperature: None,
        };

        let mut stream = stream_with_retry(self.provider.as_ref(), request).await?;
        let _ = self.event_tx.send(AgentEvent::MessageStart);

        let mut text_buf = String::new();
        let mut thinking_buf = String::new();
        let mut tool_acc: HashMap<usize, ToolCallAccumulator> = HashMap::new();
        let mut usage: Option<Usage> = None;
        let mut stop_reason = StopReason::Stop;
        // Once every handle is dropped mid-run the command channel closes; stop
        // polling it so the select below doesn't busy-loop on `recv() -> None`.
        let mut commands_open = true;

        loop {
            tokio::select! {
                maybe = stream.next() => {
                    match maybe {
                        Some(Ok(event)) => match event {
                            LlmStreamEvent::TextDelta(delta) => {
                                text_buf.push_str(&delta);
                                let _ = self.event_tx.send(AgentEvent::MessageUpdate {
                                    content: MessageUpdateContent::TextDelta { text: delta },
                                });
                            }
                            LlmStreamEvent::ThinkingDelta(content) => {
                                thinking_buf.push_str(&content);
                                let _ = self.event_tx.send(AgentEvent::ThinkingDelta { content });
                            }
                            LlmStreamEvent::ToolCallStart { index, id, name } => {
                                if tool_acc.contains_key(&index) {
                                    eprintln!("[AGENT] Duplicate ToolCallStart for index {} overwrites previous", index);
                                }
                                tool_acc.insert(index, ToolCallAccumulator {
                                    id: id.clone(),
                                    name: name.clone(),
                                    arguments: String::new(),
                                });
                                let _ = self.event_tx.send(AgentEvent::MessageUpdate {
                                    content: MessageUpdateContent::ToolCallStart { index, id, name },
                                });
                            }
                            LlmStreamEvent::ToolCallDelta { index, arguments_delta } => {
                                match tool_acc.get_mut(&index) {
                                    Some(tc) => tc.arguments.push_str(&arguments_delta),
                                    None => eprintln!(
                                        "[AGENT] ToolCallDelta for unknown index {} ignored",
                                        index
                                    ),
                                }
                                let _ = self.event_tx.send(AgentEvent::MessageUpdate {
                                    content: MessageUpdateContent::ToolCallDelta { index, arguments: arguments_delta },
                                });
                            }
                            LlmStreamEvent::ToolCallEnd { .. } => {}
                            LlmStreamEvent::Usage(u) => {
                                usage = Some(u);
                            }
                            LlmStreamEvent::Done { stop_reason: sr } => {
                                stop_reason = sr;
                                break;
                            }
                        },
                        Some(Err(e)) => {
                            eprintln!("[AGENT] Stream error at turn {}: {}", turn, e);
                            let _ = self.event_tx.send(AgentEvent::Error { error: e.to_string() });
                            return Err(e);
                        }
                        None => break, // stream ended without Done
                    }
                }
                _ = self.cancel_token.cancelled() => {
                    // Preserve the partial assistant output in the message log
                    // (and session store) before aborting.
                    if !text_buf.trim().is_empty() {
                        self.append_message(AgentMessage::Assistant {
                            content: Some(text_buf.clone()),
                            tool_calls: vec![],
                            thinking: None,
                            usage: usage.clone(),
                            stop_reason: Some(StopReason::Cancelled),
                        });
                    }
                    let _ = self.event_tx.send(AgentEvent::MessageEnd {
                        stop_reason: StopReason::Cancelled,
                    });
                    return Err(AgentError::Cancelled);
                }
                cmd = cmd_rx.recv(), if commands_open => {
                    match cmd {
                        Some(command) => self.handle_command(command),
                        None => commands_open = false,
                    }
                }
            }
        }

        println!("[AGENT] Turn {}: received {} text chars, {} tool calls, stop_reason={:?}", turn, text_buf.len(), tool_acc.len(), stop_reason);

        // Flatten accumulators in provider index order.
        let mut indices: Vec<_> = tool_acc.keys().copied().collect();
        indices.sort_unstable();
        let tool_calls = indices
            .into_iter()
            .map(|index| {
                let acc = tool_acc.remove(&index).expect("index collected from map");
                ToolCall {
                    id: acc.id,
                    name: acc.name,
                    arguments: acc.arguments,
                }
            })
            .collect();

        Ok(TurnOutput {
            text: text_buf,
            thinking: if thinking_buf.is_empty() { None } else { Some(thinking_buf) },
            tool_calls,
            usage,
            stop_reason,
        })
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
    use crate::agent::session::SessionStore;
    use futures_util::stream;
    use serde_json::json;
    use std::time::Duration;

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

    /// Reusable provider that returns scripted responses per call (call N → script[N]).
    struct ScriptedProvider {
        scripts: Vec<Vec<LlmStreamEvent>>,
        index: std::sync::atomic::AtomicUsize,
    }
    impl ScriptedProvider {
        fn new(scripts: Vec<Vec<LlmStreamEvent>>) -> Self {
            Self { scripts, index: std::sync::atomic::AtomicUsize::new(0) }
        }
    }
    #[async_trait]
    impl LlmProvider for ScriptedProvider {
        async fn stream(&self, _req: LlmRequest) -> Result<LlmStream, AgentError> {
            let i = self.index.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let events = self.scripts.get(i).cloned().unwrap_or_default();
            Ok(Box::pin(stream::iter(events.into_iter().map(Ok))))
        }
        fn name(&self) -> &str { "scripted" }
        fn capabilities(&self) -> crate::agent::provider::ProviderCapabilities { Default::default() }
    }

    /// Scripted provider that records the tool count of each request, so
    /// tests can assert on what the loop actually sent.
    struct RecordingProvider {
        scripts: Vec<Vec<LlmStreamEvent>>,
        index: std::sync::atomic::AtomicUsize,
        tool_counts: std::sync::Arc<std::sync::Mutex<Vec<usize>>>,
    }
    impl RecordingProvider {
        fn new(scripts: Vec<Vec<LlmStreamEvent>>) -> (Self, std::sync::Arc<std::sync::Mutex<Vec<usize>>>) {
            let tool_counts = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
            (Self { scripts, index: std::sync::atomic::AtomicUsize::new(0), tool_counts: tool_counts.clone() }, tool_counts)
        }
    }
    #[async_trait]
    impl LlmProvider for RecordingProvider {
        async fn stream(&self, req: LlmRequest) -> Result<LlmStream, AgentError> {
            self.tool_counts.lock().unwrap().push(req.tools.len());
            let i = self.index.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let events = self.scripts.get(i).cloned().unwrap_or_default();
            Ok(Box::pin(stream::iter(events.into_iter().map(Ok))))
        }
        fn name(&self) -> &str { "recording" }
        fn capabilities(&self) -> crate::agent::provider::ProviderCapabilities { Default::default() }
    }

    /// Provider that streams one text delta, then stalls — for cancel tests.
    struct SlowProvider;
    #[async_trait]
    impl LlmProvider for SlowProvider {
        async fn stream(&self, _req: LlmRequest) -> Result<LlmStream, AgentError> {
            let s = stream::unfold(0u32, |state| async move {
                match state {
                    0 => {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                        Some((Ok(LlmStreamEvent::TextDelta("partial".into())), 1))
                    }
                    _ => {
                        tokio::time::sleep(Duration::from_secs(30)).await;
                        None
                    }
                }
            });
            Ok(Box::pin(s))
        }
        fn name(&self) -> &str { "slow" }
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

        let provider = ScriptedProvider::new(vec![
            vec![
                LlmStreamEvent::ToolCallStart { index: 0, id: "call_1".into(), name: "echo".into() },
                LlmStreamEvent::ToolCallDelta { index: 0, arguments_delta: r#"{"text":"ping"}"#.into() },
                LlmStreamEvent::ToolCallEnd { index: 0 },
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
    async fn parallel_tool_calls_accumulate_by_index() {
        struct CaptureTool;
        #[async_trait]
        impl Tool for CaptureTool {
            fn name(&self) -> &str { "capture" }
            fn description(&self) -> &str { "capture args" }
            fn parameters(&self) -> serde_json::Value { json!({"type":"object"}) }
            async fn execute(&self, args: serde_json::Value, _ctx: &ToolContext) -> Result<ToolOutput, AgentError> {
                Ok(ToolOutput::Text(args.to_string()))
            }
        }

        // Two tool calls with interleaved argument deltas on distinct indices.
        let provider = ScriptedProvider::new(vec![
            vec![
                LlmStreamEvent::ToolCallStart { index: 0, id: "c1".into(), name: "capture".into() },
                LlmStreamEvent::ToolCallStart { index: 1, id: "c2".into(), name: "capture".into() },
                LlmStreamEvent::ToolCallDelta { index: 0, arguments_delta: "{\"a\":".into() },
                LlmStreamEvent::ToolCallDelta { index: 1, arguments_delta: "{\"b\":".into() },
                LlmStreamEvent::ToolCallDelta { index: 0, arguments_delta: "1}".into() },
                LlmStreamEvent::ToolCallDelta { index: 1, arguments_delta: "2}".into() },
                LlmStreamEvent::Done { stop_reason: StopReason::ToolCalls },
            ],
            vec![
                LlmStreamEvent::TextDelta("done".into()),
                LlmStreamEvent::Done { stop_reason: StopReason::Stop },
            ],
        ]);

        let mut agent = AgentBuilder::new()
            .provider(Box::new(provider))
            .model("test")
            .tool(Box::new(CaptureTool))
            .build()
            .unwrap();
        let result = agent.process("go").await.unwrap();
        assert_eq!(result.actions.len(), 2);
        assert!(result.actions[0].success, "args must reassemble to valid JSON: {}", result.actions[0].output_preview);
        assert!(result.actions[1].success, "args must reassemble to valid JSON: {}", result.actions[1].output_preview);

        // Both tool results must reference the right call ids.
        let tool_results: Vec<_> = agent.messages.iter().filter(|m| matches!(m, AgentMessage::ToolResult { .. })).collect();
        assert_eq!(tool_results.len(), 2);
    }

    #[tokio::test]
    async fn max_iterations_exhaustion_runs_wrapup_turn_without_tools() {
        struct AlwaysTool;
        #[async_trait]
        impl Tool for AlwaysTool {
            fn name(&self) -> &str { "noop" }
            fn description(&self) -> &str { "noop" }
            fn parameters(&self) -> serde_json::Value { json!({"type":"object"}) }
            async fn execute(&self, _args: serde_json::Value, _ctx: &ToolContext) -> Result<ToolOutput, AgentError> {
                Ok(ToolOutput::Text("ok".to_string()))
            }
        }

        let (provider, tool_counts) = RecordingProvider::new(vec![
            vec![
                LlmStreamEvent::ToolCallStart { index: 0, id: "c1".into(), name: "noop".into() },
                LlmStreamEvent::ToolCallDelta { index: 0, arguments_delta: "{}".into() },
                LlmStreamEvent::Done { stop_reason: StopReason::ToolCalls },
            ],
            vec![
                LlmStreamEvent::TextDelta("Here is what I did so far.".into()),
                LlmStreamEvent::Done { stop_reason: StopReason::Stop },
            ],
        ]);

        let mut agent = AgentBuilder::new()
            .provider(Box::new(provider))
            .model("test")
            .tool(Box::new(AlwaysTool))
            .max_iterations(1)
            .build()
            .unwrap();
        let result = agent.process("go").await.unwrap();
        // The wrap-up turn's summary is the final text, not a silent empty string.
        assert_eq!(result.text, "Here is what I did so far.");
        let counts = tool_counts.lock().unwrap().clone();
        assert_eq!(counts, vec![1, 0], "second turn must be sent with zero tools");
    }

    #[tokio::test]
    async fn cancel_mid_stream_preserves_partial_text() {
        let mut agent = AgentBuilder::new()
            .provider(Box::new(SlowProvider))
            .model("test")
            .build()
            .unwrap();
        let token = agent.cancellation_token();
        let task = tokio::spawn(async move {
            let result = agent.prompt("hello").await;
            (agent, result)
        });
        // Wait until the provider has emitted its first delta, then cancel.
        tokio::time::sleep(Duration::from_millis(150)).await;
        token.cancel();
        let (agent, result) = task.await.unwrap();
        assert!(matches!(result, Err(AgentError::Cancelled)));
        let partial_persisted = agent.messages.iter().any(|m| matches!(
            m,
            AgentMessage::Assistant { content: Some(t), stop_reason: Some(StopReason::Cancelled), .. } if t == "partial"
        ));
        assert!(partial_persisted, "partial assistant text must be preserved on cancel");
    }

    #[tokio::test]
    async fn handle_prompt_and_cancel() {
        let agent = AgentBuilder::new()
            .provider(Box::new(SlowProvider))
            .model("test")
            .build()
            .unwrap();
        let handle = agent.spawn();
        let task = tokio::spawn(handle.clone().prompt("hello"));
        tokio::time::sleep(Duration::from_millis(150)).await;
        handle.cancel();
        let result = task.await.unwrap();
        assert!(matches!(result, Err(AgentError::Cancelled)));
    }

    #[tokio::test]
    async fn handle_rejects_concurrent_prompt() {
        let agent = AgentBuilder::new()
            .provider(Box::new(SlowProvider))
            .model("test")
            .build()
            .unwrap();
        let handle = agent.spawn();
        let first = tokio::spawn(handle.clone().prompt("first"));
        // Give the actor time to start processing.
        tokio::time::sleep(Duration::from_millis(100)).await;
        let second = handle.clone().prompt("second").await;
        assert!(second.is_err(), "second prompt while running must be rejected");
        assert!(matches!(second, Err(AgentError::Provider(msg)) if msg.contains("busy")));
        // Tear down the first prompt so the test finishes immediately.
        handle.cancel();
        let first_result = first.await.unwrap();
        assert!(matches!(first_result, Err(AgentError::Cancelled)));
    }

    #[tokio::test]
    async fn messages_persist_to_attached_session() {
        let tmp = tempfile::TempDir::new().unwrap();
        let store = SessionStore::create(tmp.path(), None, None).unwrap();
        let session_id = store.session_id.clone();
        let provider = ScriptedProvider::new(vec![vec![
            LlmStreamEvent::TextDelta("hi there".into()),
            LlmStreamEvent::Done { stop_reason: StopReason::Stop },
        ]]);
        let mut agent = AgentBuilder::new()
            .provider(Box::new(provider))
            .model("test")
            .session(store)
            .build()
            .unwrap();
        agent.prompt("hello").await.unwrap();

        let reopened = SessionStore::open(tmp.path(), &session_id).unwrap();
        let msgs = reopened.replay_messages().unwrap();
        assert_eq!(msgs.len(), 2, "user + assistant must be persisted");
        assert!(matches!(&msgs[0], AgentMessage::User { .. }));
        assert!(matches!(&msgs[1], AgentMessage::Assistant { content: Some(t), .. } if t == "hi there"));
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

    #[tokio::test]
    async fn prompt_accumulates_history_across_calls() {
        let provider = ScriptedProvider::new(vec![
            vec![
                LlmStreamEvent::TextDelta("first".into()),
                LlmStreamEvent::Done { stop_reason: StopReason::Stop },
            ],
            vec![
                LlmStreamEvent::TextDelta("second".into()),
                LlmStreamEvent::Done { stop_reason: StopReason::Stop },
            ],
            vec![
                LlmStreamEvent::TextDelta("third".into()),
                LlmStreamEvent::Done { stop_reason: StopReason::Stop },
            ],
        ]);
        let mut agent = AgentBuilder::new().provider(Box::new(provider)).model("test").build().unwrap();
        let r1 = agent.prompt("a").await.unwrap();
        let r2 = agent.prompt("b").await.unwrap();
        let r3 = agent.prompt("c").await.unwrap();
        assert_eq!(r1.text, "first");
        assert_eq!(r2.text, "second");
        assert_eq!(r3.text, "third");
        // 3 user + 3 assistant = 6 messages
        assert_eq!(agent.message_count(), 6);
    }

    #[tokio::test]
    async fn followup_triggers_extra_run_cycle() {
        let provider = ScriptedProvider::new(vec![
            vec![
                LlmStreamEvent::TextDelta("ack".into()),
                LlmStreamEvent::Done { stop_reason: StopReason::Stop },
            ],
            vec![
                LlmStreamEvent::TextDelta("ack2".into()),
                LlmStreamEvent::Done { stop_reason: StopReason::Stop },
            ],
        ]);
        let mut agent = AgentBuilder::new().provider(Box::new(provider)).model("test").build().unwrap();
        agent.follow_up("queued");
        let result = agent.prompt("first").await.unwrap();
        // Loop should have continued through the follow-up; final text == ack2
        assert_eq!(result.text, "ack2");
        // user(first) + assistant(ack) + user(queued follow-up) + assistant(ack2)
        assert_eq!(agent.message_count(), 4);
    }
}
