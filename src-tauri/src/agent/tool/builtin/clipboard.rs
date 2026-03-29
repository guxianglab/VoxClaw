use async_trait::async_trait;
use serde_json::{json, Value};
use std::time::Duration;

use crate::agent::error::AgentError;
use crate::agent::tool::{Tool, ToolContext, ToolOutput};

pub struct ClipboardTool;

#[async_trait]
impl Tool for ClipboardTool {
    fn name(&self) -> &str {
        "clipboard_paste"
    }

    fn description(&self) -> &str {
        "Set the system clipboard to the given text, then simulate Ctrl+V to paste."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "text": { "type": "string", "description": "The text to paste" }
            },
            "required": ["text"]
        })
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<ToolOutput, AgentError> {
        let text = args["text"]
            .as_str()
            .unwrap_or("")
            .to_string();

        let text_clone = text.clone();
        let text_len = text.len();

        let result = tokio::task::spawn_blocking(move || {
            use arboard::Clipboard;
            use enigo::{Enigo, Key, Keyboard, Settings};

            let mut clipboard = Clipboard::new()
                .map_err(|e| AgentError::Tool(format!("Clipboard init failed: {:?}", e)))?;
            let original = clipboard.get_text().ok();

            clipboard
                .set_text(&text_clone)
                .map_err(|e| AgentError::Tool(format!("Set clipboard failed: {:?}", e)))?;

            std::thread::sleep(Duration::from_millis(10));

            let mut enigo = Enigo::new(&Settings::default())
                .map_err(|e| AgentError::Tool(format!("Enigo init failed: {:?}", e)))?;

            enigo
                .key(Key::Control, enigo::Direction::Press)
                .map_err(|e| AgentError::Tool(format!("Ctrl press failed: {:?}", e)))?;
            std::thread::sleep(Duration::from_millis(5));
            enigo
                .key(Key::Unicode('v'), enigo::Direction::Click)
                .map_err(|e| AgentError::Tool(format!("V click failed: {:?}", e)))?;
            std::thread::sleep(Duration::from_millis(5));
            enigo
                .key(Key::Control, enigo::Direction::Release)
                .map_err(|e| AgentError::Tool(format!("Ctrl release failed: {:?}", e)))?;

            // Restore original clipboard after paste
            std::thread::sleep(Duration::from_millis(100));
            if let Some(orig) = original {
                let _ = clipboard.set_text(&orig);
            }

            Ok::<(), AgentError>(())
        })
        .await
        .map_err(|e| AgentError::Tool(format!("Paste task panicked: {}", e)))?;

        match result {
            Ok(()) => Ok(ToolOutput::Text(format!(
                "Pasted {} characters",
                text_len
            ))),
            Err(e) => Ok(ToolOutput::Error(e.to_string())),
        }
    }
}
