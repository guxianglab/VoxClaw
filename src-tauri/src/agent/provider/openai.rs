use async_trait::async_trait;
use futures_util::StreamExt;
use serde_json::{json, Value};
use std::collections::HashMap;
use tokio::sync::mpsc;

use crate::agent::core::request::*;
use crate::agent::core::usage::Usage;
use crate::agent::error::AgentError;
use crate::agent::provider::sse::{data_payload, SseBuffer};
use crate::agent::provider::{LlmProvider, LlmStream, ProviderCapabilities};
use crate::http_client::build_client;
use crate::state::preview_text;
use crate::storage::ProxyConfig;

pub struct OpenAiProvider {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
    _model: String,
}

impl OpenAiProvider {
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
}

fn response_preview(text: &str, max_chars: usize) -> String {
    preview_text(text, max_chars)
}

/// Per-response accumulator for a streaming chat completion.
#[derive(Default)]
struct StreamState {
    /// Tool-call index -> (id, name). OpenAI streams the id/name only on the
    /// first delta of each tool call; arguments trickle in afterwards.
    tool_calls: HashMap<usize, (String, String)>,
    stop_reason: Option<StopReason>,
    usage: Option<Usage>,
}

/// Map one SSE `data:` JSON chunk onto stream events.
fn parse_chunk(value: &Value, state: &mut StreamState) -> Vec<LlmStreamEvent> {
    let mut events = Vec::new();

    // Usage, when the provider includes it on the final chunk.
    if let Some(usage) = value.get("usage").filter(|u| !u.is_null()) {
        let input = usage.get("prompt_tokens").and_then(Value::as_u64).unwrap_or(0);
        let output = usage.get("completion_tokens").and_then(Value::as_u64).unwrap_or(0);
        let cache_read = usage
            .get("prompt_tokens_details")
            .and_then(|d| d.get("cached_tokens"))
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let total = usage.get("total_tokens").and_then(Value::as_u64).unwrap_or(0);
        state.usage = Some(Usage {
            input,
            output,
            cache_read,
            cache_write: 0,
            total,
            ..Default::default()
        });
    }

    let Some(choice) = value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|c| c.first())
    else {
        return events;
    };

    if let Some(finish_reason) = choice.get("finish_reason").and_then(Value::as_str) {
        if !finish_reason.is_empty() {
            state.stop_reason = Some(StopReason::from_openai(finish_reason));
        }
    }

    let delta = choice.get("delta");
    if let Some(content) = delta
        .and_then(|d| d.get("content"))
        .and_then(|c| if c.is_null() { None } else { c.as_str() })
        .filter(|s| !s.is_empty())
    {
        events.push(LlmStreamEvent::TextDelta(content.to_string()));
    }

    if let Some(reasoning) = delta
        .and_then(|d| d.get("reasoning_content"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    {
        // DeepSeek-style reasoning field; surfaced through the generic
        // ThinkingDelta channel.
        events.push(LlmStreamEvent::ThinkingDelta(reasoning.to_string()));
    }

    if let Some(tool_calls) = delta.and_then(|d| d.get("tool_calls")).and_then(Value::as_array) {
        for tc in tool_calls {
            let Some(index) = tc.get("index").and_then(Value::as_u64) else {
                continue;
            };
            let index = index as usize;
            let id = tc.get("id").and_then(Value::as_str).unwrap_or("");
            let name = tc
                .get("function")
                .and_then(|f| f.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let arguments = tc
                .get("function")
                .and_then(|f| f.get("arguments"))
                .and_then(Value::as_str)
                .unwrap_or("");

            if !id.is_empty() || (!name.is_empty() && !state.tool_calls.contains_key(&index)) {
                let entry = state.tool_calls.entry(index).or_insert_with(|| (String::new(), String::new()));
                if !id.is_empty() {
                    entry.0 = id.to_string();
                }
                if !name.is_empty() {
                    entry.1 = name.to_string();
                }
                events.push(LlmStreamEvent::ToolCallStart {
                    index,
                    id: entry.0.clone(),
                    name: entry.1.clone(),
                });
            }
            if !arguments.is_empty() {
                events.push(LlmStreamEvent::ToolCallDelta {
                    index,
                    arguments_delta: arguments.to_string(),
                });
            }
        }
    }

    events
}

/// Emit the terminating events (per-call End + Usage + Done) once the stream
/// finishes, whether via `[DONE]` or socket close.
fn finalize_events(state: &mut StreamState) -> Vec<LlmStreamEvent> {
    let mut events = Vec::new();
    let mut indices: Vec<_> = state.tool_calls.keys().copied().collect();
    indices.sort_unstable();
    for index in indices {
        events.push(LlmStreamEvent::ToolCallEnd { index });
    }
    if let Some(usage) = state.usage.take() {
        events.push(LlmStreamEvent::Usage(usage));
    }
    events.push(LlmStreamEvent::Done {
        stop_reason: state.stop_reason.take().unwrap_or(StopReason::Stop),
    });
    events
}

#[async_trait]
impl LlmProvider for OpenAiProvider {
    fn name(&self) -> &str {
        "openai_compatible"
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            thinking: false,
            vision: true,
            tool_calling: true,
            streaming: true,
        }
    }

    async fn stream(&self, request: LlmRequest) -> Result<LlmStream, AgentError> {
        let url = format!("{}/chat/completions", self.base_url);

        let mut body = json!({
            "model": request.model,
            "messages": request.messages.iter().map(|m| {
                let mut obj = json!({ "role": m.role });
                if let Some(content) = &m.content {
                    obj["content"] = json!(content);
                }
                if !m.tool_calls.is_empty() {
                    obj["tool_calls"] = json!(m.tool_calls);
                }
                if let Some(id) = &m.tool_call_id {
                    obj["tool_call_id"] = json!(id);
                }
                obj
            }).collect::<Vec<_>>(),
            "stream": true,
        });

        if !request.tools.is_empty() {
            body["tools"] = json!(request.tools);
            body["tool_choice"] = json!("auto");
        }
        if let Some(max_tokens) = request.max_tokens {
            body["max_tokens"] = json!(max_tokens);
        }
        if let Some(temp) = request.temperature {
            body["temperature"] = json!(temp);
        }

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| AgentError::Provider(format!("Request failed: {}", e)))?;

        let status = response.status();
        if !status.is_success() {
            let response_text = response.text().await.unwrap_or_default();
            let preview = response_preview(&response_text, 400);
            return Err(AgentError::Provider(format!(
                "API error ({}): {}",
                status, preview
            )));
        }

        // Parse the SSE body on a dedicated task and forward events through a
        // channel. Dropping the returned stream drops the receiver, which ends
        // the task and aborts the underlying HTTP connection — that is what
        // makes mid-stream cancellation immediate.
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
                    if data == "[DONE]" {
                        for event in finalize_events(&mut state) {
                            if tx.send(Ok(event)).await.is_err() {
                                return;
                            }
                        }
                        return;
                    }
                    let Ok(value) = serde_json::from_str::<Value>(data) else {
                        continue;
                    };
                    for event in parse_chunk(&value, &mut state) {
                        if tx.send(Ok(event)).await.is_err() {
                            return;
                        }
                    }
                }
            }
            // Socket closed without [DONE]: still terminate the stream so the
            // loop sees a Done event instead of hanging on a silent end.
            for event in finalize_events(&mut state) {
                if tx.send(Ok(event)).await.is_err() {
                    return;
                }
            }
        });

        Ok(Box::pin(futures_util::stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|item| (item, rx))
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_preview_handles_multibyte_text() {
        assert_eq!(response_preview("你好，世界", 4), "你好，世");
    }

    fn chunk(json: &str) -> Value {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn parse_chunk_text_delta() {
        let mut state = StreamState::default();
        let events = parse_chunk(
            &chunk(r#"{"choices":[{"delta":{"content":"你好"}}]}"#),
            &mut state,
        );
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], LlmStreamEvent::TextDelta(t) if t == "你好"));
    }

    #[test]
    fn parse_chunk_tool_call_start_and_delta() {
        let mut state = StreamState::default();
        let events = parse_chunk(
            &chunk(r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"echo","arguments":""}}]}}]}"#),
            &mut state,
        );
        assert!(matches!(&events[0], LlmStreamEvent::ToolCallStart { index, id, name }
            if *index == 0 && id == "call_1" && name == "echo"));

        // Arguments trickle in on later chunks without id/name.
        let events = parse_chunk(
            &chunk(r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"text\":"}}]}}]}"#),
            &mut state,
        );
        assert!(matches!(&events[0], LlmStreamEvent::ToolCallDelta { index, arguments_delta }
            if *index == 0 && arguments_delta.contains("text")));

        // A second parallel tool call gets its own index.
        let events = parse_chunk(
            &chunk(r#"{"choices":[{"delta":{"tool_calls":[{"index":1,"id":"call_2","function":{"name":"bash"}}]}}]}"#),
            &mut state,
        );
        assert!(matches!(&events[0], LlmStreamEvent::ToolCallStart { index, .. } if *index == 1));
    }

    #[test]
    fn finalize_emits_ends_usage_done() {
        let mut state = StreamState::default();
        state.tool_calls.insert(1, ("c2".into(), "bash".into()));
        state.tool_calls.insert(0, ("c1".into(), "echo".into()));
        state.stop_reason = Some(StopReason::ToolCalls);
        state.usage = Some(Usage {
            input: 10,
            output: 5,
            total: 15,
            ..Default::default()
        });
        let events = finalize_events(&mut state);
        // Ends sorted by index, then usage, then done.
        assert!(matches!(&events[0], LlmStreamEvent::ToolCallEnd { index } if *index == 0));
        assert!(matches!(&events[1], LlmStreamEvent::ToolCallEnd { index } if *index == 1));
        assert!(matches!(&events[2], LlmStreamEvent::Usage(u) if u.input == 10));
        assert!(matches!(&events[3], LlmStreamEvent::Done { stop_reason } if *stop_reason == StopReason::ToolCalls));
    }

    #[test]
    fn finalize_without_stop_reason_defaults_to_stop() {
        let mut state = StreamState::default();
        let events = finalize_events(&mut state);
        assert!(matches!(&events[0], LlmStreamEvent::Done { stop_reason } if *stop_reason == StopReason::Stop));
    }
}
