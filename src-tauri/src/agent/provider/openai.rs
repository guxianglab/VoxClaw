use async_trait::async_trait;
use serde_json::{json, Value};

use crate::agent::core::request::*;
use crate::agent::error::AgentError;
use crate::agent::provider::{LlmProvider, LlmStream, ProviderCapabilities};
use crate::http_client::build_client;
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
            "stream": false,
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

        // println!("[AGENT] LLM request: POST {}, model={}", url, request.model);
        // println!("[AGENT] LLM request body: {}", serde_json::to_string(&body).unwrap_or_default());

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
        let response_text = response.text().await.unwrap_or_default();
        println!("[AGENT] LLM response status: {}", status);
        println!("[AGENT] LLM response body (first 2000 chars): {}", &response_text[..response_text.len().min(2000)]);

        if !status.is_success() {
            return Err(AgentError::Provider(format!(
                "API error ({}): {}",
                status, response_text
            )));
        }

        // Convert bytes stream to SSE events
        // For now, collect the full response and parse as non-streaming
        // SSE streaming will be added in the stream.rs task
        let response_json: Value = serde_json::from_str(&response_text)
            .map_err(|e| AgentError::Provider(format!("Parse error: {} | raw response: {}", e, &response_text[..response_text.len().min(500)])))?;

        let events = parse_chat_response(&response_json);
        Ok(Box::pin(
            futures_util::stream::iter(events.into_iter().map(Ok)),
        ))
    }
}

fn parse_chat_response(value: &Value) -> Vec<LlmStreamEvent> {
    let mut events = Vec::new();

    let choice = match value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|c| c.first())
    {
        Some(c) => c,
        None => return events,
    };

    let message = match choice.get("message") {
        Some(m) => m,
        None => return events,
    };

    // Text content
    if let Some(content) = message
        .get("content")
        .and_then(|c| if c.is_null() { None } else { c.as_str() })
        .filter(|s| !s.is_empty())
    {
        events.push(LlmStreamEvent::TextDelta(content.to_string()));
    }

    // Tool calls
    if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array) {
        for (index, tc) in tool_calls.iter().enumerate() {
            let id = tc
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let name = tc
                .get("function")
                .and_then(|f| f.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let arguments = tc
                .get("function")
                .and_then(|f| f.get("arguments"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();

            events.push(LlmStreamEvent::ToolCallStart {
                index,
                id,
                name,
            });
            if !arguments.is_empty() {
                events.push(LlmStreamEvent::ToolCallDelta {
                    index,
                    arguments_delta: arguments,
                });
            }
            events.push(LlmStreamEvent::ToolCallEnd { _index: index });
        }
    }

    // Stop reason
    let stop_reason = choice
        .get("finish_reason")
        .and_then(Value::as_str)
        .map(StopReason::from_openai)
        .unwrap_or(StopReason::Stop);
    events.push(LlmStreamEvent::Done { stop_reason });

    events
}
