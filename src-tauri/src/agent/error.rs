use thiserror::Error;

#[derive(Debug, Error)]
pub enum AgentError {
    #[error("Provider error: {0}")]
    Provider(String),

    #[error("Tool error: {0}")]
    Tool(String),

    #[error("Cancelled")]
    Cancelled,

    #[error("Max iterations reached ({0})")]
    #[allow(dead_code)]
    MaxIterationsReached(u32),

    #[error("Context error: {0}")]
    #[allow(dead_code)]
    Context(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Unknown tool: {0}")]
    #[allow(dead_code)]
    UnknownTool(String),

    #[error("Invalid tool arguments: {0}")]
    InvalidToolArguments(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_error_displays_correctly() {
        let err = AgentError::Provider("API returned 429".into());
        assert_eq!(err.to_string(), "Provider error: API returned 429");
    }

    #[test]
    fn cancelled_error_displays_correctly() {
        let err = AgentError::Cancelled;
        assert_eq!(err.to_string(), "Cancelled");
    }

    #[test]
    fn serialization_error_converts() {
        let json_err = serde_json::from_str::<serde_json::Value>("not json");
        let err = AgentError::from(json_err.unwrap_err());
        let msg = err.to_string();
        assert!(msg.contains("Serialization error"));
    }
}
