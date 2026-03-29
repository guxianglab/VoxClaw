use async_trait::async_trait;
use serde_json::{json, Value};
use std::time::Duration;
use tokio::process::Command;

use crate::agent::error::AgentError;
use crate::agent::tool::{Tool, ToolContext, ToolOutput};

pub struct BashTool;

#[async_trait]
impl Tool for BashTool {
    fn name(&self) -> &str {
        "bash"
    }

    fn description(&self) -> &str {
        "Execute a shell command. Use cmd /C on Windows. Returns stdout/stderr."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The shell command to execute"
                },
                "timeout_ms": {
                    "type": "integer",
                    "description": "Timeout in milliseconds (default 30000)",
                    "default": 30000
                },
                "cwd": {
                    "type": "string",
                    "description": "Working directory for the command (optional)"
                }
            },
            "required": ["command"]
        })
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<ToolOutput, AgentError> {
        let command = args["command"]
            .as_str()
            .unwrap_or("");
        let timeout_ms = args["timeout_ms"].as_u64().unwrap_or(30_000);
        let cwd = args["cwd"].as_str();

        let mut cmd = Command::new("cmd");
        cmd.args(["/C", command]);
        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }

        let output = tokio::time::timeout(Duration::from_millis(timeout_ms), cmd.output())
            .await
            .map_err(|_| AgentError::Tool(format!(
                "Command timed out after {}ms",
                timeout_ms
            )))?
            .map_err(|e| AgentError::Tool(format!("Failed to execute command: {}", e)))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let exit_code = output.status.code().unwrap_or(-1);

        let mut result = String::new();
        if !stdout.is_empty() {
            result.push_str(&stdout);
        }
        if !stderr.is_empty() {
            if !result.is_empty() {
                result.push('\n');
            }
            result.push_str("[stderr] ");
            result.push_str(&stderr);
        }
        result.push_str(&format!("\n[exit code: {}]", exit_code));

        if output.status.success() {
            Ok(ToolOutput::Text(result.trim().to_string()))
        } else {
            Ok(ToolOutput::Error(result.trim().to_string()))
        }
    }
}
