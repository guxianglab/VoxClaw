use async_trait::async_trait;
use serde_json::{json, Value};

use crate::agent::error::AgentError;
use crate::agent::tool::{Tool, ToolContext, ToolOutput};

pub struct FileEditTool;

#[async_trait]
impl Tool for FileEditTool {
    fn name(&self) -> &str {
        "file_edit"
    }

    fn description(&self) -> &str {
        "Perform exact string replacement in a file."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "file_path": { "type": "string", "description": "Path to the file" },
                "old_string": { "type": "string", "description": "Exact string to find" },
                "new_string": { "type": "string", "description": "Replacement string" },
                "replace_all": { "type": "boolean", "description": "Replace all occurrences (default false)" }
            },
            "required": ["file_path", "old_string", "new_string"]
        })
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<ToolOutput, AgentError> {
        let path = args["file_path"]
            .as_str()
            .ok_or_else(|| AgentError::InvalidToolArguments("file_path required".into()))?;
        let old = args["old_string"]
            .as_str()
            .ok_or_else(|| AgentError::InvalidToolArguments("old_string required".into()))?;
        let new = args["new_string"]
            .as_str()
            .ok_or_else(|| AgentError::InvalidToolArguments("new_string required".into()))?;
        let replace_all = args["replace_all"].as_bool().unwrap_or(false);

        let content = tokio::fs::read_to_string(path)
            .await
            .map_err(|e| AgentError::Tool(format!("Read failed: {}", e)))?;

        if !content.contains(old) {
            return Err(AgentError::Tool("old_string not found in file".into()));
        }

        let new_content = if replace_all {
            content.replace(old, new)
        } else {
            content.replacen(old, new, 1)
        };

        tokio::fs::write(path, &new_content)
            .await
            .map_err(|e| AgentError::Tool(format!("Write failed: {}", e)))?;

        let count = if replace_all {
            content.matches(old).count()
        } else {
            1
        };

        Ok(ToolOutput::Text(format!(
            "Replaced {} occurrence(s) in {}",
            count, path
        )))
    }
}
