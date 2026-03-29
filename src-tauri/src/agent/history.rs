use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

const MAX_ENTRIES: usize = 50;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentHistoryEntry {
    pub timestamp: String,
    pub user_input: String,
    pub agent_output: String,
    pub tool_summaries: Vec<String>,
}

fn history_path(data_dir: &Path) -> PathBuf {
    data_dir.join("agent_history.jsonl")
}

/// Append an entry and trim to MAX_ENTRIES.
pub fn append_entry(data_dir: &Path, entry: &AgentHistoryEntry) -> std::io::Result<()> {
    let path = history_path(data_dir);
    // Ensure parent exists
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    // Read existing entries
    let mut entries: Vec<AgentHistoryEntry> = if path.exists() {
        let content = fs::read_to_string(&path)?;
        content
            .lines()
            .filter(|line| !line.trim().is_empty())
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect()
    } else {
        Vec::new()
    };

    entries.push(entry.clone());

    // Trim oldest if over limit
    if entries.len() > MAX_ENTRIES {
        let drain_count = entries.len() - MAX_ENTRIES;
        entries.drain(..drain_count);
    }

    // Write all entries back
    let mut content = String::new();
    for e in &entries {
        if let Ok(line) = serde_json::to_string(e) {
            content.push_str(&line);
            content.push('\n');
        }
    }
    fs::write(&path, content)
}

/// Read the N most recent entries.
pub fn read_recent(data_dir: &Path, count: usize) -> Vec<AgentHistoryEntry> {
    let path = history_path(data_dir);
    if !path.exists() {
        return Vec::new();
    }

    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let entries: Vec<AgentHistoryEntry> = content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();

    let skip = if entries.len() > count { entries.len() - count } else { 0 };
    entries.into_iter().skip(skip).collect()
}

/// Format recent entries as a context string to inject into system prompt.
pub fn format_as_context(entries: &[AgentHistoryEntry]) -> String {
    if entries.is_empty() {
        return String::new();
    }

    let mut ctx = String::from("\n## Recent Conversations\n");
    for (i, entry) in entries.iter().enumerate() {
        ctx.push_str(&format!(
            "{}. [{}] User: \"{}\" → Agent: \"{}\"\n",
            i + 1,
            entry.timestamp,
            entry.user_input.chars().take(100).collect::<String>(),
            entry.agent_output.chars().take(100).collect::<String>()
        ));
    }
    ctx
}
