use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::process::Command;

use crate::agent::error::AgentError;
use crate::agent::tool::{Tool, ToolContext, ToolOutput};

pub struct OpenUrlTool;

#[async_trait]
impl Tool for OpenUrlTool {
    fn name(&self) -> &str {
        "open_url"
    }

    fn description(&self) -> &str {
        "Open a URL in the default browser."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "The URL to open" }
            },
            "required": ["url"]
        })
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<ToolOutput, AgentError> {
        let url = args["url"].as_str().unwrap_or("");
        let mut cmd = Command::new("cmd");
        cmd.args(["/C", "start", "", url]);
        let status = cmd
            .status()
            .await
            .map_err(|e| AgentError::Tool(format!("Failed to open URL: {}", e)))?;

        if status.success() {
            Ok(ToolOutput::Text(format!("Opened {}", url)))
        } else {
            Ok(ToolOutput::Error("Failed to open URL".into()))
        }
    }
}
