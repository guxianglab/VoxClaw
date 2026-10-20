use async_trait::async_trait;
use futures_util::StreamExt;
use serde_json::{json, Value};
use std::collections::HashSet;
use tokio::sync::mpsc;

use crate::agent::core::request::*;
use crate::agent::core::usage::Usage;
use crate::agent::error::AgentError;
use crate::agent::provider::sse::{data_payload, SseBuffer};
use crate::agent::provider::{LlmProvider, LlmStream, ProviderCapabilities};
use crate::http_client::build_client;
use crate::state::preview_text;
use crate::storage::ProxyConfig;

pub struct AnthropicProvider {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
    _model: String,
}

impl AnthropicProvider {
    pub fn new(
        base_url: &str,
        api_key: &str,
        model: &str,
        proxy: &ProxyConfig,
    ) -> Result<Self, AgentError> {
        let client =
            build_client(proxy, 120).map_err(|e| AgentError::Provider(e.to_string()))?;
        Ok(Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            _model: model.to_string(),
        })
    }

    fn response_preview(text: &str, max_chars: usize) -> String {
        preview_text(text, max_chars)
    }

    /// Convert internal LlmMessages into Anthropic API format.
    /// Returns (system_prompt, api_messages).
    /// Key differences from OpenAI:
    /// - `system` is extracted as a top-level field, not in the messages array
    /// - User/assistant messages use content block arrays
    /// - Tool results are appended to the last user message
    /// - Assistant tool calls use `tool_use` content blocks
    fn convert_messages(&self, messages: &[LlmMessage]) -> (Option<String>, Vec<Value>) {
        let mut system = None;
        let mut api_messages = Vec::new();

        for msg in messages {
            match msg.role.as_str() {
                "system" => {
                    system = msg.content.clone();
                }
                "user" => {
                    let mut content = Vec::new();
                    if let Some(text) = &msg.content {
                        content.push(json!({ "type": "text", "text": text }));
                    }
                    api_messages.push(json!({ "role": "user", "content": content }));
                }
                "assistant" => {
                    let mut content = Vec::new();
                    if let Some(text) = &msg.content {
                        if !text.is_empty() {
                            content.push(json!({ "type": "text", "text": text }));
                        }
                    }
                    if !msg.tool_calls.is_empty() {
                        for tc in &msg.tool_calls {
                            content.push(json!({
                                "type": "tool_use",
                                "id": tc.id,
                                "name": tc.function.name,
                                "input": serde_json::from_str::<Value>(&tc.function.arguments)
                                    .unwrap_or(json!({}))
                            }));
                        }
                    }
                    api_messages.push(json!({ "role": "assistant", "content": content }));
                }
                "tool" => {
                    // Anthropic expects tool_result content blocks inside user messages.
                    // If the last message is a user message, append to it; otherwise create a new user message.
                    if let Some(text) = &msg.content {
                        let tool_result = json!({
                            "type": "tool_result",
                            "tool_use_id": msg.tool_call_id.as_deref().unwrap_or(""),
                            "content": text
                        });
                        if let Some(last) = api_messages.last_mut() {
                            if last["role"] == "user" {
                                if let Some(arr) =
                                    last.get_mut("content").and_then(|c| c.as_array_mut())
                                {
                                    arr.push(tool_result);
                                    continue;
                                }
                            }
                        }
                        // No existing user message to append to; create a new one
                        api_messages.push(json!({ "role": "user", "content": [tool_result] }));
                    }
                }
                _ => {}
            }
        }

        (system, api_messages)
    }
}

/// Per-response accumulator for an Anthropic SSE stream.
#[derive(Default)]
struct StreamState {
    /// Content-block indices that are `tool_use` blocks.
    tool_indices: HashSet<usize>,
    stop_reason: Option<StopReason>,
    usage: Usage,
    has_usage: bool,
}

fn stop_reason_from_anthropic(reason: &str) -> StopReason {
    match reason {
        "end_turn" | "stop_sequence" => StopReason::Stop,
        "tool_use" => StopReason::ToolCalls,
        "max_tokens" => StopReason::MaxTokens,
        "refusal" => StopReason::Refusal,
        other => StopReason::Error(other.to_string()),
    }
}

fn read_usage(usage: &Value, target: &mut Usage) {
    if let Some(v) = usage.get("input_tokens").and_then(Value::as_u64) {
        target.input = v;
    }
    if let Some(v) = usage.get("output_tokens").and_then(Value::as_u64) {
        target.output = v;
    }
    if let Some(v) = usage.get("cache_read_input_tokens").and_then(Value::as_u64) {
        target.cache_read = v;
    }
    if let Some(v) = usage.get("cache_creation_input_tokens").and_then(Value::as_u64) {
        target.cache_write = v;
    }
}

/// Map one SSE `data:` JSON payload onto stream events.
/// Returns `Err` for in-stream API errors so the run aborts with the message.
fn parse_event(
    value: &Value,
    state: &mut StreamState,
) -> Result<Vec<LlmStreamEvent>, AgentError> {
    let mut events = Vec::new();
    let event_type = value.get("type").and_then(Value::as_str).unwrap_or("");

    match event_type {
        "message_start" => {
            if let Some(usage) = value.get("message").and_then(|m| m.get("usage")) {
                read_usage(usage, &mut state.usage);
                state.has_usage = true;
            }
        }
        "content_block_start" => {
            let index = value.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
            if let Some(block) = value.get("content_block") {
                if block.get("type").and_then(Value::as_str) == Some("tool_use") {
                    state.tool_indices.insert(index);
                    events.push(LlmStreamEvent::ToolCallStart {
                        index,
                        id: block.get("id").and_then(Value::as_str).unwrap_or("").to_string(),
                        name: block
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string(),
                    });
                }
            }
        }
        "content_block_delta" => {
            let index = value.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
            let delta_type = value
                .get("delta")
                .and_then(|d| d.get("type"))
                .and_then(Value::as_str)
                .unwrap_or("");
            match delta_type {
                "text_delta" => {
                    if let Some(text) = value
                        .get("delta")
                        .and_then(|d| d.get("text"))
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty())
                    {
                        events.push(LlmStreamEvent::TextDelta(text.to_string()));
                    }
                }
                "thinking_delta" => {
                    if let Some(text) = value
                        .get("delta")
                        .and_then(|d| d.get("thinking"))
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty())
                    {
                        events.push(LlmStreamEvent::ThinkingDelta(text.to_string()));
                    }
                }
                "input_json_delta" => {
                    if let Some(partial) = value
                        .get("delta")
                        .and_then(|d| d.get("partial_json"))
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty())
                    {
                        events.push(LlmStreamEvent::ToolCallDelta {
                            index,
                            arguments_delta: partial.to_string(),
                        });
                    }
                }
                _ => {}
            }
        }
        "content_block_stop" => {
            let index = value.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
            if state.tool_indices.contains(&index) {
                events.push(LlmStreamEvent::ToolCallEnd { index });
            }
        }
        "message_delta" => {
            if let Some(reason) = value
                .get("delta")
                .and_then(|d| d.get("stop_reason"))
                .and_then(Value::as_str)
            {
                state.stop_reason = Some(stop_reason_from_anthropic(reason));
            }
            if let Some(usage) = value.get("usage") {
                read_usage(usage, &mut state.usage);
                state.has_usage = true;
            }
        }
        "message_stop" => {
            // Terminal event: flush usage + done.
            let mut indices: Vec<_> = state.tool_indices.iter().copied().collect();
            indices.sort_unstable();
            for index in indices {
                if !events
                    .iter()
                    .any(|e| matches!(e, LlmStreamEvent::ToolCallEnd { index: i } if *i == index))
                {
                    events.push(LlmStreamEvent::ToolCallEnd { index });
                }
            }
            if state.has_usage {
                let mut usage = state.usage.clone();
                usage.total = usage.input + usage.output;
                events.push(LlmStreamEvent::Usage(usage));
            }
            events.push(LlmStreamEvent::Done {
                stop_reason: state.stop_reason.take().unwrap_or(StopReason::Stop),
            });
        }
        "error" => {
            let message = value
                .get("error")
                .and_then(|e| e.get("message"))
                .and_then(Value::as_str)
                .unwrap_or("unknown Anthropic stream error");
            return Err(AgentError::Provider(format!(
                "Anthropic stream error: {}",
                message
            )));
        }
        _ => {} // ping and unknown events are ignored
    }

    Ok(events)
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    fn name(&self) -> &str {
        "anthropic"
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            thinking: true,
            vision: true,
            tool_calling: true,
            streaming: true,
        }
    }

    async fn stream(&self, request: LlmRequest) -> Result<LlmStream, AgentError> {
        let url = format!("{}/v1/messages", self.base_url);
        let (system, messages) = self.convert_messages(&request.messages);

        let mut body = json!({
            "model": request.model,
            "messages": messages,
            "max_tokens": request.max_tokens.unwrap_or(4096),
            "stream": true,
        });

        if let Some(sys) = system {
            body["system"] = json!(sys);
        }

        if !request.tools.is_empty() {
            // Anthropic tool format: no `type: "function"` wrapper, uses `input_schema` instead of `parameters`
            let anthropic_tools: Vec<Value> = request
                .tools
                .iter()
                .map(|t| {
                    json!({
                        "name": t.function.name,
                        "description": t.function.description,
                        "input_schema": t.function.parameters
                    })
                })
                .collect();
            body["tools"] = json!(anthropic_tools);
            body["tool_choice"] = json!({ "type": "auto" });
        }

        if let Some(thinking) = &request.thinking {
            if thinking.level.is_active() {
                let budget = thinking
                    .budget_tokens
                    .or_else(|| ThinkingBudgets::default().budget_for(thinking.level))
                    .unwrap_or(1024);
                body["thinking"] = json!({
                    "type": "enabled",
                    "budget_tokens": budget
                });
            }
        }

        let response = self
            .client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| AgentError::Provider(format!("Request failed: {}", e)))?;

        let status = response.status();
        if !status.is_success() {
            let response_text = response.text().await.unwrap_or_default();
            let preview = Self::response_preview(&response_text, 400);
            return Err(AgentError::Provider(format!(
                "API error ({}): {}",
                status, preview
            )));
        }

        // Same pattern as the OpenAI provider: parse SSE on a dedicated task;
        // dropping the returned stream aborts the HTTP connection.
        let (tx, rx) = mpsc::channel::<Result<LlmStreamEvent, AgentError>>(64);
        tokio::spawn(async move {
            let mut sse = SseBuffer::new();
            let mut state = StreamState::default();
            let mut body = response.bytes_stream();
            while let Some(chunk) = body.next().await {
                let bytes = match chunk {
                    Ok(b) => b,
                    Err(e) => {
                        let _ = tx
                            .send(Err(AgentError::Provider(format!(
                                "Stream read error: {}",
                                e
                            ))))
                            .await;
                        return;
                    }
                };
                sse.push_chunk(&bytes);
                while let Some(line) = sse.next_line() {
                    let Some(data) = data_payload(&line) else {
                        continue;
                    };
                    let Ok(value) = serde_json::from_str::<Value>(data) else {
                        continue;
                    };
                    match parse_event(&value, &mut state) {
                        Ok(events) => {
                            for event in events {
                                if tx.send(Ok(event)).await.is_err() {
                                    return;
                                }
                            }
                        }
                        Err(e) => {
                            let _ = tx.send(Err(e)).await;
                            return;
                        }
                    }
                }
            }
            // Socket closed without message_stop: terminate the stream anyway.
            let mut indices: Vec<_> = state.tool_indices.iter().copied().collect();
            indices.sort_unstable();
            for index in indices {
                let _ = tx.send(Ok(LlmStreamEvent::ToolCallEnd { index })).await;
            }
            let _ = tx
                .send(Ok(LlmStreamEvent::Done {
                    stop_reason: state.stop_reason.take().unwrap_or(StopReason::Stop),
                }))
                .await;
        });

        Ok(Box::pin(futures_util::stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|item| (item, rx))
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::provider::create_provider;

    #[test]
    fn anthropic_provider_creation() {
        let provider = AnthropicProvider::new(
            "https://api.anthropic.com",
            "test-key",
            "claude-sonnet-4-20250514",
            &Default::default(),
        );
        assert!(provider.is_ok());
        assert_eq!(provider.unwrap().name(), "anthropic");
    }

    #[test]
    fn anthropic_capabilities() {
        let provider = AnthropicProvider::new(
            "https://api.anthropic.com",
            "k",
            "m",
            &Default::default(),
        )
        .unwrap();
        let caps = provider.capabilities();
        assert!(caps.thinking);
        assert!(caps.vision);
        assert!(caps.tool_calling);
        assert!(caps.streaming);
    }

    #[test]
    fn create_provider_factory_anthropic() {
        let provider = create_provider(
            "anthropic",
            "https://api.anthropic.com",
            "test-key",
            "claude-sonnet-4-20250514",
            &Default::default(),
        );
        assert!(provider.is_ok());
        assert_eq!(provider.unwrap().name(), "anthropic");
    }

    #[test]
    fn convert_messages_extracts_system() {
        let provider = AnthropicProvider::new("https://api.anthropic.com", "k", "m", &Default::default()).unwrap();
        let messages = vec![
            LlmMessage::system("You are helpful."),
            LlmMessage::user("Hello"),
        ];
        let (system, api_msgs) = provider.convert_messages(&messages);
        assert_eq!(system, Some("You are helpful.".to_string()));
        assert_eq!(api_msgs.len(), 1);
        assert_eq!(api_msgs[0]["role"], "user");
    }

    #[test]
    fn convert_messages_tool_results_create_user_message() {
        let provider = AnthropicProvider::new("https://api.anthropic.com", "k", "m", &Default::default()).unwrap();
        let messages = vec![
            LlmMessage::user("What is the weather?"),
            LlmMessage::assistant_tool_calls(vec![
                LlmToolCall {
                    id: "call_1".into(),
                    call_type: "function".into(),
                    function: LlmFunctionCall {
                        name: "get_weather".into(),
                        arguments: r#"{"city":"NYC"}"#.into(),
                    },
                },
            ]),
            LlmMessage::tool_result("call_1", "Sunny, 72F"),
        ];
        let (_system, api_msgs) = provider.convert_messages(&messages);
        // Should have: user, assistant (with tool_use), new user (with tool_result)
        assert_eq!(api_msgs.len(), 3);

        // The first user message should contain only the text
        let user_content = api_msgs[0]["content"].as_array().unwrap();
        assert_eq!(user_content.len(), 1);
        assert_eq!(user_content[0]["type"], "text");

        // The assistant message should contain a tool_use block
        let asst_content = api_msgs[1]["content"].as_array().unwrap();
        assert_eq!(asst_content.len(), 1);
        assert_eq!(asst_content[0]["type"], "tool_use");
        assert_eq!(asst_content[0]["name"], "get_weather");

        // The second user message should contain the tool_result
        let result_content = api_msgs[2]["content"].as_array().unwrap();
        assert_eq!(result_content.len(), 1);
        assert_eq!(result_content[0]["type"], "tool_result");
        assert_eq!(result_content[0]["tool_use_id"], "call_1");
    }

    fn event(json: &str) -> Value {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn parse_stream_text_and_stop() {
        let mut state = StreamState::default();
        let events = parse_event(
            &event(r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello!"}}"#),
            &mut state,
        )
        .unwrap();
        assert!(matches!(&events[0], LlmStreamEvent::TextDelta(t) if t == "Hello!"));

        let events = parse_event(
            &event(r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":7}}"#),
            &mut state,
        )
        .unwrap();
        assert!(events.is_empty());

        let events = parse_event(&event(r#"{"type":"message_stop"}"#), &mut state).unwrap();
        assert!(matches!(&events[0], LlmStreamEvent::Usage(u) if u.output == 7));
        assert!(matches!(&events[1], LlmStreamEvent::Done { stop_reason } if *stop_reason == StopReason::Stop));
    }

    #[test]
    fn parse_stream_parallel_tool_use_uses_block_indices() {
        let mut state = StreamState::default();

        // Two tool_use blocks at indices 0 and 1 — the old non-streaming
        // parser hardcoded index 0 for both and corrupted the arguments.
        let events = parse_event(
            &event(r#"{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"tu_1","name":"file_read","input":{}}}"#),
            &mut state,
        )
        .unwrap();
        assert!(matches!(&events[0], LlmStreamEvent::ToolCallStart { index, id, name } if *index == 0 && id == "tu_1" && name == "file_read"));

        let events = parse_event(
            &event(r#"{"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"tu_2","name":"bash","input":{}}}"#),
            &mut state,
        )
        .unwrap();
        assert!(matches!(&events[0], LlmStreamEvent::ToolCallStart { index, id, .. } if *index == 1 && id == "tu_2"));

        let events = parse_event(
            &event(r#"{"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"command\":"}}"#),
            &mut state,
        )
        .unwrap();
        assert!(matches!(&events[0], LlmStreamEvent::ToolCallDelta { index, arguments_delta }
            if *index == 1 && arguments_delta.contains("command")));

        let events = parse_event(
            &event(r#"{"type":"content_block_stop","index":0}"#),
            &mut state,
        )
        .unwrap();
        assert!(matches!(&events[0], LlmStreamEvent::ToolCallEnd { index } if *index == 0));

        let events = parse_event(
            &event(r#"{"type":"message_delta","delta":{"stop_reason":"tool_use"}}"#),
            &mut state,
        )
        .unwrap();
        assert!(events.is_empty());

        let events = parse_event(&event(r#"{"type":"message_stop"}"#), &mut state).unwrap();
        let end_idx = events.iter().position(|e| matches!(e, LlmStreamEvent::ToolCallEnd { index } if *index == 1));
        assert!(end_idx.is_some(), "index 1 End must be emitted at message_stop");
        let done = events.iter().find(|e| matches!(e, LlmStreamEvent::Done { .. }));
        assert!(matches!(done, Some(LlmStreamEvent::Done { stop_reason }) if *stop_reason == StopReason::ToolCalls));
    }

    #[test]
    fn parse_stream_thinking_delta() {
        let mut state = StreamState::default();
        let events = parse_event(
            &event(r#"{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"Let me think..."}}"#),
            &mut state,
        )
        .unwrap();
        assert!(matches!(&events[0], LlmStreamEvent::ThinkingDelta(t) if t == "Let me think..."));
    }

    #[test]
    fn parse_stream_error_event_maps_to_err() {
        let mut state = StreamState::default();
        let result = parse_event(
            &event(r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#),
            &mut state,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Overloaded"));
    }

    #[test]
    fn parse_stream_ping_ignored() {
        let mut state = StreamState::default();
        let events = parse_event(&event(r#"{"type":"ping"}"#), &mut state).unwrap();
        assert!(events.is_empty());
    }

    #[test]
    fn response_preview_handles_multibyte_text() {
        assert_eq!(AnthropicProvider::response_preview("检查中文边界", 5), "检查中文边");
    }
}
