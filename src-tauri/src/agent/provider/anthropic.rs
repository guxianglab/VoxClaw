use async_trait::async_trait;
use futures_util::stream;
use serde_json::{json, Value};

use crate::agent::core::request::*;
use crate::agent::error::AgentError;
use crate::agent::provider::{LlmProvider, LlmStream, ProviderCapabilities};
use crate::http_client::build_client;
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

    /// Parse Anthropic Messages API response into stream events.
    /// Handles text, thinking, and tool_use content blocks.
    fn parse_response(&self, body: &Value) -> Vec<LlmStreamEvent> {
        let mut events = Vec::new();

        if let Some(content) = body.get("content").and_then(Value::as_array) {
            for block in content {
                let btype = block.get("type").and_then(Value::as_str).unwrap_or("");

                if btype == "text" {
                    if let Some(text) = block.get("text").and_then(Value::as_str) {
                        events.push(LlmStreamEvent::TextDelta(text.to_string()));
                    }
                } else if btype == "thinking" {
                    if let Some(text) = block.get("thinking").and_then(Value::as_str) {
                        events.push(LlmStreamEvent::ThinkingDelta(text.to_string()));
                    }
                } else if btype == "tool_use" {
                    let id = block
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    let name = block
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    let input = block.get("input").cloned().unwrap_or(json!({}));

                    events.push(LlmStreamEvent::ToolCallStart {
                        index: 0,
                        id,
                        name,
                    });
                    events.push(LlmStreamEvent::ToolCallDelta {
                        index: 0,
                        arguments_delta: input.to_string(),
                    });
                    events.push(LlmStreamEvent::ToolCallEnd { _index: 0 });
                }
            }
        }

        let stop_reason = match body.get("stop_reason").and_then(Value::as_str) {
            Some("end_turn") => StopReason::Stop,
            Some("tool_use") => StopReason::ToolCalls,
            Some("max_tokens") => StopReason::MaxTokens,
            _ => StopReason::Stop,
        };
        events.push(LlmStreamEvent::Done { stop_reason });

        events
    }
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
            streaming: false, // Native Anthropic SSE uses a different event format; deferred to Phase 4
        }
    }

    async fn stream(&self, request: LlmRequest) -> Result<LlmStream, AgentError> {
        let url = format!("{}/v1/messages", self.base_url);
        let (system, messages) = self.convert_messages(&request.messages);

        let mut body = json!({
            "model": request.model,
            "messages": messages,
            "max_tokens": request.max_tokens.unwrap_or(4096),
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
            if thinking.level != ThinkingLevel::None {
                body["thinking"] = json!({
                    "type": "enabled",
                    "budget_tokens": thinking.budget_tokens.unwrap_or(10000)
                });
            }
        }

        println!("[AGENT] Anthropic request: POST {}, model={}", url, request.model);
        println!("[AGENT] Anthropic request body: {}", serde_json::to_string(&body).unwrap_or_default());

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
        let response_text = response.text().await.unwrap_or_default();
        println!("[AGENT] Anthropic response status: {}", status);
        println!("[AGENT] Anthropic response body (first 2000 chars): {}", &response_text[..response_text.len().min(2000)]);

        if !status.is_success() {
            return Err(AgentError::Provider(format!(
                "API error ({}): {}",
                status, response_text
            )));
        }

        let body: Value = serde_json::from_str(&response_text)
            .map_err(|e| AgentError::Provider(format!("Parse error: {} | raw response: {}", e, &response_text[..response_text.len().min(500)])))?;

        let events = self.parse_response(&body);
        Ok(Box::pin(stream::iter(events.into_iter().map(Ok))))
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
        assert!(!caps.streaming); // non-streaming for now
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

    #[test]
    fn parse_response_text_and_stop() {
        let provider = AnthropicProvider::new("https://api.anthropic.com", "k", "m", &Default::default()).unwrap();
        let body = json!({
            "content": [{"type": "text", "text": "Hello!"}],
            "stop_reason": "end_turn"
        });
        let events = provider.parse_response(&body);
        assert_eq!(events.len(), 2);
        assert!(matches!(&events[0], LlmStreamEvent::TextDelta(t) if t == "Hello!"));
        assert!(matches!(&events[1], LlmStreamEvent::Done { stop_reason } if *stop_reason == StopReason::Stop));
    }

    #[test]
    fn parse_response_tool_use() {
        let provider = AnthropicProvider::new("https://api.anthropic.com", "k", "m", &Default::default()).unwrap();
        let body = json!({
            "content": [{"type": "tool_use", "id": "tu_1", "name": "read_file", "input": {"path": "/tmp/test"}}],
            "stop_reason": "tool_use"
        });
        let events = provider.parse_response(&body);
        assert!(matches!(&events[0], LlmStreamEvent::ToolCallStart { id, name, .. } if id == "tu_1" && name == "read_file"));
        assert!(matches!(&events[3], LlmStreamEvent::Done { stop_reason } if *stop_reason == StopReason::ToolCalls));
    }

    #[test]
    fn parse_response_thinking() {
        let provider = AnthropicProvider::new("https://api.anthropic.com", "k", "m", &Default::default()).unwrap();
        let body = json!({
            "content": [
                {"type": "thinking", "thinking": "Let me think..."},
                {"type": "text", "text": "Here is my answer."}
            ],
            "stop_reason": "end_turn"
        });
        let events = provider.parse_response(&body);
        assert_eq!(events.len(), 3);
        assert!(matches!(&events[0], LlmStreamEvent::ThinkingDelta(t) if t == "Let me think..."));
        assert!(matches!(&events[1], LlmStreamEvent::TextDelta(t) if t == "Here is my answer."));
    }
}
