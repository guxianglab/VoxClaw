pub mod anthropic;
pub mod openai;
pub mod sse;

use async_trait::async_trait;
use std::time::Duration;

use crate::agent::core::request::{LlmRequest, LlmStream};
use crate::agent::error::AgentError;
use crate::storage::ProxyConfig;

/// Provider capabilities declaration.
#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub struct ProviderCapabilities {
    pub thinking: bool,
    pub vision: bool,
    pub tool_calling: bool,
    pub streaming: bool,
}

/// Unified LLM provider trait.
#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn stream(&self, request: LlmRequest) -> Result<LlmStream, AgentError>;
    #[allow(dead_code)]
    fn name(&self) -> &str;
    #[allow(dead_code)]
    fn capabilities(&self) -> ProviderCapabilities;
}

/// Create a provider by type string.
pub fn create_provider(
    provider_type: &str,
    base_url: &str,
    api_key: &str,
    model: &str,
    proxy: &ProxyConfig,
) -> Result<Box<dyn LlmProvider>, AgentError> {
    match provider_type {
        "anthropic" => {
            anthropic::AnthropicProvider::new(base_url, api_key, model, proxy)
                .map(|p| Box::new(p) as Box<dyn LlmProvider>)
        }
        "openai_compatible" | "deepseek" | "ollama" | "groq" => {
            openai::OpenAiProvider::new(base_url, api_key, model, proxy)
                .map(|p| Box::new(p) as Box<dyn LlmProvider>)
        }
        other => Err(AgentError::Provider(format!(
            "Unknown provider type: {}",
            other
        ))),
    }
}

/// Heuristic: should this provider error be retried? Only failures that occur
/// before any content is streamed (connection errors, HTTP status errors) are
/// retryable — those are the only failures this helper sees, since it wraps
/// request creation, not stream consumption.
fn is_retryable_error(err: &AgentError) -> bool {
    let AgentError::Provider(msg) = err else {
        return false;
    };
    // Network-level failures surface as "Request failed: ..." / timeouts.
    if msg.starts_with("Request failed") || msg.contains("timed out") || msg.contains("timeout") {
        return true;
    }
    // Retryable HTTP statuses surfaced by the providers as "API error ({status}): ...".
    for code in ["429", "500", "502", "503", "504", "529"] {
        if msg.contains(&format!("({}", code)) {
            return true;
        }
    }
    false
}

const RETRY_MAX_ATTEMPTS: usize = 3;
const RETRY_BASE_DELAY: Duration = Duration::from_millis(500);

/// Issue a provider request with exponential backoff on transient failures
/// (rate limits, server errors, network errors). Retries only cover the
/// initial request: once events start flowing, errors propagate immediately.
pub async fn stream_with_retry(
    provider: &dyn LlmProvider,
    request: LlmRequest,
) -> Result<LlmStream, AgentError> {
    let mut attempt = 0usize;
    loop {
        match provider.stream(request.clone()).await {
            Ok(stream) => return Ok(stream),
            Err(e) => {
                if attempt + 1 >= RETRY_MAX_ATTEMPTS || !is_retryable_error(&e) {
                    return Err(e);
                }
                let delay = RETRY_BASE_DELAY * (1 << attempt) as u32;
                eprintln!(
                    "[AGENT] Provider request failed (attempt {}), retrying in {:?}: {}",
                    attempt + 1,
                    delay,
                    e
                );
                tokio::time::sleep(delay).await;
                attempt += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_capabilities_default() {
        let caps = ProviderCapabilities::default();
        assert!(!caps.thinking);
        assert!(!caps.vision);
    }

    #[test]
    fn openai_provider_factory() {
        let provider = create_provider(
            "openai_compatible",
            "https://api.openai.com/v1",
            "test-key",
            "gpt-4o",
            &Default::default(),
        );
        assert!(provider.is_ok());
        assert_eq!(provider.unwrap().name(), "openai_compatible");
    }

    #[test]
    fn retryable_errors_are_recognized() {
        assert!(is_retryable_error(&AgentError::Provider(
            "API error (429 Too Many Requests): rate limited".into()
        )));
        assert!(is_retryable_error(&AgentError::Provider(
            "API error (503 Service Unavailable): overloaded".into()
        )));
        assert!(is_retryable_error(&AgentError::Provider(
            "Request failed: error sending request for url".into()
        )));
        assert!(!is_retryable_error(&AgentError::Provider(
            "API error (401 Unauthorized): bad key".into()
        )));
        assert!(!is_retryable_error(&AgentError::Provider(
            "Parse error: bad json".into()
        )));
        assert!(!is_retryable_error(&AgentError::Cancelled));
    }
}
