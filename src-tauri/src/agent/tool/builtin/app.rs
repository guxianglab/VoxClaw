use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::process::Command;

use crate::agent::error::AgentError;
use crate::agent::tool::{Tool, ToolContext, ToolOutput};

// ---------------------------------------------------------------------------
// OpenCalculatorTool
// ---------------------------------------------------------------------------

pub struct OpenCalculatorTool;

#[async_trait]
impl Tool for OpenCalculatorTool {
    fn name(&self) -> &str {
        "open_calculator"
    }

    fn description(&self) -> &str {
        "Open the Windows Calculator app."
    }

    fn parameters(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }

    async fn execute(&self, _args: Value, _ctx: &ToolContext) -> Result<ToolOutput, AgentError> {
        let status = Command::new("calc.exe")
            .status()
            .await
            .map_err(|e| AgentError::Tool(format!("Failed to open calculator: {}", e)))?;

        if status.success() {
            Ok(ToolOutput::Text("Calculator opened".into()))
        } else {
            Ok(ToolOutput::Error("Failed to open calculator".into()))
        }
    }
}

// ---------------------------------------------------------------------------
// OpenBrowserTool
// ---------------------------------------------------------------------------

pub struct OpenBrowserTool;

#[async_trait]
impl Tool for OpenBrowserTool {
    fn name(&self) -> &str {
        "open_browser"
    }

    fn description(&self) -> &str {
        "Open the default web browser. Optionally navigate to a URL."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "URL to open (optional)" }
            }
        })
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<ToolOutput, AgentError> {
        let url = args["url"].as_str().unwrap_or("");
        let mut cmd = Command::new("cmd");
        if url.is_empty() {
            cmd.args(["/C", "start", ""]);
        } else {
            cmd.args(["/C", "start", "", url]);
        }
        let status = cmd
            .status()
            .await
            .map_err(|e| AgentError::Tool(format!("Failed to open browser: {}", e)))?;

        let output = if url.is_empty() {
            "Browser opened".into()
        } else {
            format!("Opened browser at {}", url)
        };

        if status.success() {
            Ok(ToolOutput::Text(output))
        } else {
            Ok(ToolOutput::Error("Failed to open browser".into()))
        }
    }
}

// ---------------------------------------------------------------------------
// OpenNotepadTool
// ---------------------------------------------------------------------------

pub struct OpenNotepadTool;

#[async_trait]
impl Tool for OpenNotepadTool {
    fn name(&self) -> &str {
        "open_notepad"
    }

    fn description(&self) -> &str {
        "Open Windows Notepad."
    }

    fn parameters(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }

    async fn execute(&self, _args: Value, _ctx: &ToolContext) -> Result<ToolOutput, AgentError> {
        let status = Command::new("notepad.exe")
            .status()
            .await
            .map_err(|e| AgentError::Tool(format!("Failed to open notepad: {}", e)))?;

        if status.success() {
            Ok(ToolOutput::Text("Notepad opened".into()))
        } else {
            Ok(ToolOutput::Error("Failed to open notepad".into()))
        }
    }
}

// ---------------------------------------------------------------------------
// OpenExplorerTool
// ---------------------------------------------------------------------------

pub struct OpenExplorerTool;

#[async_trait]
impl Tool for OpenExplorerTool {
    fn name(&self) -> &str {
        "open_explorer"
    }

    fn description(&self) -> &str {
        "Open Windows File Explorer. Optionally open at a specific path."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Directory path to open (optional)" }
            }
        })
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<ToolOutput, AgentError> {
        let path = args["path"].as_str().unwrap_or("");
        let mut cmd = Command::new("explorer.exe");
        if !path.is_empty() {
            cmd.arg(path);
        }
        let status = cmd
            .status()
            .await
            .map_err(|e| AgentError::Tool(format!("Failed to open explorer: {}", e)))?;

        let output = if path.is_empty() {
            "Explorer opened".into()
        } else {
            format!("Opened explorer at {}", path)
        };

        if status.success() {
            Ok(ToolOutput::Text(output))
        } else {
            Ok(ToolOutput::Error("Failed to open explorer".into()))
        }
    }
}

// ---------------------------------------------------------------------------
// ScreenshotTool
// ---------------------------------------------------------------------------

pub struct ScreenshotTool;

#[async_trait]
impl Tool for ScreenshotTool {
    fn name(&self) -> &str {
        "screenshot"
    }

    fn description(&self) -> &str {
        "Open the Windows Snipping Tool for taking a screenshot."
    }

    fn parameters(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }

    async fn execute(&self, _args: Value, _ctx: &ToolContext) -> Result<ToolOutput, AgentError> {
        let status = Command::new("SnippingTool.exe")
            .status()
            .await
            .map_err(|e| AgentError::Tool(format!("Failed to open Snipping Tool: {}", e)))?;

        if status.success() {
            Ok(ToolOutput::Text("Snipping Tool opened".into()))
        } else {
            Ok(ToolOutput::Error("Failed to open Snipping Tool".into()))
        }
    }
}

// ---------------------------------------------------------------------------
// ComposeEmailTool
// ---------------------------------------------------------------------------

pub struct ComposeEmailTool;

#[async_trait]
impl Tool for ComposeEmailTool {
    fn name(&self) -> &str {
        "compose_email"
    }

    fn description(&self) -> &str {
        "Open the default email client with a pre-filled email. Use mailto: protocol."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "to": { "type": "string", "description": "Recipient email address (optional)" },
                "subject": { "type": "string", "description": "Email subject (optional)" },
                "body": { "type": "string", "description": "Email body (optional)" }
            }
        })
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<ToolOutput, AgentError> {
        let to = args["to"].as_str().unwrap_or("");
        let subject = args["subject"].as_str().unwrap_or("");
        let body = args["body"].as_str().unwrap_or("");

        let mut mailto = String::from("mailto:");
        mailto.push_str(to);
        if !subject.is_empty() || !body.is_empty() {
            mailto.push('?');
            let mut params = Vec::new();
            if !subject.is_empty() {
                params.push(format!("subject={}", urlencoding::encode(subject)));
            }
            if !body.is_empty() {
                params.push(format!("body={}", urlencoding::encode(body)));
            }
            mailto.push_str(&params.join("&"));
        }

        let mut cmd = Command::new("cmd");
        cmd.args(["/C", "start", "", &mailto]);
        let status = cmd
            .status()
            .await
            .map_err(|e| AgentError::Tool(format!("Failed to open email client: {}", e)))?;

        let output = if to.is_empty() {
            "Opened email client".into()
        } else {
            format!("Opened email client to {}", to)
        };

        if status.success() {
            Ok(ToolOutput::Text(output))
        } else {
            Ok(ToolOutput::Error("Failed to open email client".into()))
        }
    }
}
