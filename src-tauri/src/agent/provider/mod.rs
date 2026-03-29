pub mod anthropic;
pub mod openai;

use async_trait::async_trait;

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
}
