use async_trait::async_trait;
use serde_json::{json, Value};

use crate::agent::error::AgentError;
use crate::agent::tool::{Tool, ToolContext, ToolOutput};

pub struct FileWriteTool;

#[async_trait]
impl Tool for FileWriteTool {
    fn name(&self) -> &str {
        "file_write"
    }

    fn description(&self) -> &str {
        "Write text content to a file. Creates the file if it doesn't exist, overwrites if it does."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "File path to write to" },
                "content": { "type": "string", "description": "Content to write" }
            },
            "required": ["path", "content"]
        })
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<ToolOutput, AgentError> {
        let path = args["path"]
            .as_str()
            .unwrap_or("");
        let content = args["content"]
            .as_str()
            .unwrap_or("");

        tokio::fs::write(path, content)
            .await
            .map_err(|e| AgentError::Tool(format!("Failed to write file '{}': {}", path, e)))?;

        Ok(ToolOutput::Text(format!(
            "Wrote {} bytes to {}",
            content.len(),
            path
        )))
    }
}
