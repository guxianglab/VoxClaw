use async_trait::async_trait;
use serde_json::{json, Value};

use crate::agent::error::AgentError;
use crate::agent::tool::{Tool, ToolContext, ToolOutput};

pub struct FileReadTool;

#[async_trait]
impl Tool for FileReadTool {
    fn name(&self) -> &str {
        "file_read"
    }

    fn description(&self) -> &str {
        "Read the contents of a file. Returns up to the first 10000 characters."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Absolute or relative file path" }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<ToolOutput, AgentError> {
        let path = args["path"]
            .as_str()
            .unwrap_or("");
        let content = tokio::fs::read_to_string(path)
            .await
            .map_err(|e| AgentError::Tool(format!("Failed to read file '{}': {}", path, e)))?;

        // Truncate to prevent huge outputs
        let truncated = if content.len() > 10_000 {
            format!("{}... [truncated at 10000 chars]", &content[..10_000])
        } else {
            content
        };

        Ok(ToolOutput::Text(truncated))
    }
}
