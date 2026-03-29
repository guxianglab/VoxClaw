use async_trait::async_trait;
use serde_json::{json, Value};

use crate::agent::error::AgentError;
use crate::agent::tool::{Tool, ToolContext, ToolOutput};

pub struct GlobTool;

#[async_trait]
impl Tool for GlobTool {
    fn name(&self) -> &str {
        "glob"
    }

    fn description(&self) -> &str {
        "Find files matching a glob pattern."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string", "description": "Glob pattern (e.g. **/*.rs)" },
                "directory": { "type": "string", "description": "Root directory to search (default: current)" }
            },
            "required": ["pattern"]
        })
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<ToolOutput, AgentError> {
        let pattern = args["pattern"]
            .as_str()
            .ok_or_else(|| AgentError::InvalidToolArguments("pattern required".into()))?;
        let dir = args["directory"].as_str().unwrap_or(".");

        let mut matches = Vec::new();
        walk_dir(dir, pattern, &mut matches).await;

        let output = if matches.is_empty() {
            "No files matched.".to_string()
        } else {
            matches.join("\n")
        };

        Ok(ToolOutput::Text(output))
    }
}

async fn walk_dir(base: &str, pattern: &str, matches: &mut Vec<String>) {
    let mut entries = match tokio::fs::read_dir(base).await {
        Ok(e) => e,
        Err(_) => return,
    };

    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");
        let rel = path.to_string_lossy().to_string();

        if path.is_dir() {
            Box::pin(walk_dir(&rel, pattern, matches)).await;
        } else if glob_match::glob_match(pattern, name) || glob_match::glob_match(pattern, &rel)
        {
            matches.push(rel);
        }
    }
}
