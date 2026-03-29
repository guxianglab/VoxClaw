use async_trait::async_trait;
use regex::Regex;
use serde_json::{json, Value};

use crate::agent::error::AgentError;
use crate::agent::tool::{Tool, ToolContext, ToolOutput};

pub struct GrepTool;

#[async_trait]
impl Tool for GrepTool {
    fn name(&self) -> &str {
        "grep"
    }

    fn description(&self) -> &str {
        "Search file contents using a regex pattern."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string", "description": "Regex pattern to search for" },
                "path": { "type": "string", "description": "File or directory to search" },
                "include": { "type": "string", "description": "Glob filter for filenames (e.g. *.rs)" }
            },
            "required": ["pattern", "path"]
        })
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<ToolOutput, AgentError> {
        let pattern = args["pattern"]
            .as_str()
            .ok_or_else(|| AgentError::InvalidToolArguments("pattern required".into()))?;
        let path = args["path"]
            .as_str()
            .ok_or_else(|| AgentError::InvalidToolArguments("path required".into()))?;

        let re = Regex::new(pattern)
            .map_err(|e| AgentError::InvalidToolArguments(format!("Invalid regex: {}", e)))?;

        let metadata = tokio::fs::metadata(path)
            .await
            .map_err(|e| AgentError::Tool(format!("Cannot access '{}': {}", path, e)))?;

        let mut results = Vec::new();

        if metadata.is_file() {
            search_file(path, &re, &mut results).await;
        } else {
            search_dir(path, &re, &mut results, args["include"].as_str()).await;
        }

        let output = if results.is_empty() {
            "No matches found.".to_string()
        } else if results.len() > 100 {
            format!(
                "{} matches (showing first 100):\n{}",
                results.len(),
                results[..100].join("\n")
            )
        } else {
            results.join("\n")
        };

        Ok(ToolOutput::Text(output))
    }
}

async fn search_file(path: &str, re: &Regex, results: &mut Vec<String>) {
    let content = match tokio::fs::read_to_string(path).await {
        Ok(c) => c,
        Err(_) => return,
    };

    for (i, line) in content.lines().enumerate() {
        if re.is_match(line) {
            results.push(format!("{}:{}: {}", path, i + 1, line));
        }
    }
}

async fn search_dir(dir: &str, re: &Regex, results: &mut Vec<String>, include: Option<&str>) {
    let mut entries = match tokio::fs::read_dir(dir).await {
        Ok(e) => e,
        Err(_) => return,
    };

    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();

        if path.is_dir() {
            Box::pin(search_dir(
                path.to_str().unwrap_or(""),
                re,
                results,
                include,
            ))
            .await;
        } else {
            if let Some(filter) = include {
                let filename = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("");
                if !glob_match::glob_match(filter, filename) {
                    continue;
                }
            }
            search_file(path.to_str().unwrap_or(""), re, results).await;
        }
    }
}
