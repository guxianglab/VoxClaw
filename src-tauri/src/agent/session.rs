//! Session log: pi-mono-style append-only event stream replacing the flat
//! `agent_history.jsonl`. Each session is a JSONL file under
//! `{app_data}/sessions/{session_id}.jsonl`.
//!
//! The first line is a header (`SessionHeader`); all subsequent lines are
//! `SessionEntry` records linked by `id` / `parent_id`. Replaying the log
//! reconstructs `Vec<AgentMessage>` plus model/thinking-level changes.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::agent::core::message::AgentMessage;

/// File format version. Bump when on-disk schema changes.
pub const SESSION_FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionHeader {
    pub version: u32,
    pub session_id: String,
    pub created_at: String,
    /// Optional working dir at creation time (for diagnostics; not enforced).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// If this session was branched from another, points back to that one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_session: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SessionEntryKind {
    Message {
        message: AgentMessage,
    },
    ThinkingLevelChange {
        level: String,
    },
    ModelChange {
        provider: String,
        model: String,
    },
    Compaction {
        summary: String,
        first_kept_id: Option<String>,
        tokens_before: Option<u64>,
        details: Option<serde_json::Value>,
    },
    BranchSummary {
        from_id: String,
        summary: String,
    },
    Custom {
        custom_type: String,
        data: serde_json::Value,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionEntry {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    pub timestamp: String,
    #[serde(flatten)]
    pub kind: SessionEntryKind,
}

impl SessionEntry {
    pub fn new(parent_id: Option<String>, kind: SessionEntryKind) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            parent_id,
            timestamp: Utc::now().to_rfc3339(),
            kind,
        }
    }
}

fn sessions_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("sessions")
}

fn session_path(data_dir: &Path, session_id: &str) -> PathBuf {
    sessions_dir(data_dir).join(format!("{}.jsonl", session_id))
}

/// Append-only session store. Holds an in-memory cursor to the last entry id
/// so subsequent appends can chain `parent_id`.
pub struct SessionStore {
    pub session_id: String,
    pub data_dir: PathBuf,
    last_entry_id: Option<String>,
}

impl SessionStore {
    /// Create a brand-new session file with header. `parent_session` is set
    /// when this session is a branch of another.
    pub fn create(
        data_dir: &Path,
        cwd: Option<String>,
        parent_session: Option<String>,
    ) -> std::io::Result<Self> {
        fs::create_dir_all(sessions_dir(data_dir))?;
        let session_id = Uuid::new_v4().to_string();
        let path = session_path(data_dir, &session_id);
        let header = SessionHeader {
            version: SESSION_FORMAT_VERSION,
            session_id: session_id.clone(),
            created_at: Utc::now().to_rfc3339(),
            cwd,
            parent_session,
        };
        let mut f = OpenOptions::new().create_new(true).write(true).open(&path)?;
        let line = serde_json::to_string(&header)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        writeln!(f, "{}", line)?;
        Ok(Self {
            session_id,
            data_dir: data_dir.to_path_buf(),
            last_entry_id: None,
        })
    }

    /// Open an existing session and rewind cursor to the last entry.
    pub fn open(data_dir: &Path, session_id: &str) -> std::io::Result<Self> {
        let path = session_path(data_dir, session_id);
        if !path.exists() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("session not found: {}", session_id),
            ));
        }
        let mut last_entry_id: Option<String> = None;
        let f = fs::File::open(&path)?;
        let reader = BufReader::new(f);
        for (idx, line) in reader.lines().enumerate() {
            let line = line?;
            if idx == 0 || line.trim().is_empty() {
                continue;
            }
            if let Ok(entry) = serde_json::from_str::<SessionEntry>(&line) {
                last_entry_id = Some(entry.id);
            }
        }
        Ok(Self {
            session_id: session_id.to_string(),
            data_dir: data_dir.to_path_buf(),
            last_entry_id,
        })
    }

    pub fn path(&self) -> PathBuf {
        session_path(&self.data_dir, &self.session_id)
    }

    /// Append an entry, automatically chaining `parent_id` to the last one.
    pub fn append(&mut self, kind: SessionEntryKind) -> std::io::Result<SessionEntry> {
        let entry = SessionEntry::new(self.last_entry_id.clone(), kind);
        let line = serde_json::to_string(&entry)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        let mut f = OpenOptions::new().append(true).open(self.path())?;
        writeln!(f, "{}", line)?;
        self.last_entry_id = Some(entry.id.clone());
        Ok(entry)
    }

    /// Replay all `Message` entries into the order needed to seed an Agent.
    pub fn replay_messages(&self) -> std::io::Result<Vec<AgentMessage>> {
        let f = fs::File::open(self.path())?;
        let reader = BufReader::new(f);
        let mut out = Vec::new();
        for (idx, line) in reader.lines().enumerate() {
            let line = line?;
            if idx == 0 || line.trim().is_empty() {
                continue;
            }
            if let Ok(entry) = serde_json::from_str::<SessionEntry>(&line) {
                if let SessionEntryKind::Message { message } = entry.kind {
                    out.push(message);
                }
            }
        }
        Ok(out)
    }
}

/// Lightweight metadata for the session list UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub session_id: String,
    pub created_at: String,
    pub entry_count: usize,
    pub last_user_text: Option<String>,
    pub last_assistant_text: Option<String>,
}

/// List all sessions sorted by created_at descending.
pub fn list_sessions(data_dir: &Path) -> std::io::Result<Vec<SessionSummary>> {
    let dir = sessions_dir(data_dir);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut summaries = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
            continue;
        }
        if let Some(s) = read_summary(&path) {
            summaries.push(s);
        }
    }
    summaries.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(summaries)
}

fn read_summary(path: &Path) -> Option<SessionSummary> {
    let f = fs::File::open(path).ok()?;
    let reader = BufReader::new(f);
    let mut header: Option<SessionHeader> = None;
    let mut entry_count = 0usize;
    let mut last_user: Option<String> = None;
    let mut last_assistant: Option<String> = None;
    for (idx, line) in reader.lines().enumerate() {
        let line = line.ok()?;
        if line.trim().is_empty() {
            continue;
        }
        if idx == 0 {
            header = serde_json::from_str(&line).ok();
            continue;
        }
        if let Ok(entry) = serde_json::from_str::<SessionEntry>(&line) {
            entry_count += 1;
            if let SessionEntryKind::Message { message } = &entry.kind {
                match message {
                    AgentMessage::User { content, .. } => {
                        if let crate::agent::core::message::UserContent::Text(t) = content {
                            last_user = Some(t.clone());
                        }
                    }
                    AgentMessage::Assistant {
                        content: Some(text),
                        ..
                    } => {
                        last_assistant = Some(text.clone());
                    }
                    _ => {}
                }
            }
        }
    }
    let header = header?;
    Some(SessionSummary {
        session_id: header.session_id,
        created_at: header.created_at,
        entry_count,
        last_user_text: last_user,
        last_assistant_text: last_assistant,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn create_append_replay() {
        let tmp = TempDir::new().unwrap();
        let mut store = SessionStore::create(tmp.path(), None, None).unwrap();
        store
            .append(SessionEntryKind::Message {
                message: AgentMessage::user("hi"),
            })
            .unwrap();
        store
            .append(SessionEntryKind::Message {
                message: AgentMessage::assistant_text("hello"),
            })
            .unwrap();
        let messages = store.replay_messages().unwrap();
        assert_eq!(messages.len(), 2);
    }

    #[test]
    fn open_resumes_chain() {
        let tmp = TempDir::new().unwrap();
        let session_id = {
            let mut s = SessionStore::create(tmp.path(), None, None).unwrap();
            s.append(SessionEntryKind::Message {
                message: AgentMessage::user("first"),
            })
            .unwrap();
            s.session_id.clone()
        };
        let mut reopened = SessionStore::open(tmp.path(), &session_id).unwrap();
        let entry = reopened
            .append(SessionEntryKind::Message {
                message: AgentMessage::user("second"),
            })
            .unwrap();
        assert!(entry.parent_id.is_some(), "second entry should chain off first");
        let all = reopened.replay_messages().unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn list_sessions_sorted() {
        let tmp = TempDir::new().unwrap();
        let _a = SessionStore::create(tmp.path(), None, None).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        let _b = SessionStore::create(tmp.path(), None, None).unwrap();
        let summaries = list_sessions(tmp.path()).unwrap();
        assert_eq!(summaries.len(), 2);
        assert!(summaries[0].created_at >= summaries[1].created_at);
    }
}
