use serde::Serialize;
use std::collections::HashSet;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

use crate::asr;
use crate::audio;
use crate::input_listener;
use crate::storage;

// ---------------------------------------------------------------------------
// Tauri managed state type aliases
// ---------------------------------------------------------------------------

pub type AudioState = Mutex<audio::AudioService>;
pub type AsrState = asr::AsrService;
pub type StorageState = storage::StorageService;
pub type InputListenerState = input_listener::InputListener;
pub type ProcessingState = Arc<std::sync::atomic::AtomicBool>;
pub type LlmCancelState = Arc<Mutex<Option<CancellationToken>>>;
pub type AgentCancelState = Arc<Mutex<Option<CancellationToken>>>;
pub type SkillExecutionState = Arc<Mutex<Option<SkillExecutionSession>>>;
pub type MeetingState = Arc<Mutex<Option<crate::meeting::session::ActiveMeeting>>>;

// ---------------------------------------------------------------------------
// Static sequence counters
// ---------------------------------------------------------------------------

pub static TRANSCRIPTION_SEQ: AtomicU64 = AtomicU64::new(1);
pub static DICTATION_SESSION_SEQ: AtomicU64 = AtomicU64::new(1);
pub static SKILL_SESSION_SEQ: AtomicU64 = AtomicU64::new(1);

// ---------------------------------------------------------------------------
// Dictation state machine
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DictationIntent {
    Raw,
    Polish,
    Skill,
    Agent,
    None,
}

impl DictationIntent {
    pub fn as_event(self) -> &'static str {
        match self {
            Self::Raw => "raw",
            Self::Polish => "polish",
            Self::Skill => "skill",
            Self::Agent => "agent",
            Self::None => "none",
        }
    }
}

#[derive(Debug)]
pub struct PendingFinalizeState {
    pub session_id: u64,
    pub intent: DictationIntent,
    pub window_elapsed: bool,
    pub asr_result: Option<Result<String, String>>,
}

#[derive(Debug)]
pub enum DictationState {
    Idle,
    Recording {
        intent: DictationIntent,
        started_at: std::time::Instant,
    },
    PendingFinalize(PendingFinalizeState),
}

// ---------------------------------------------------------------------------
// Skill execution session
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
pub struct SkillExecutionSession {
    pub id: u64,
    pub executed: HashSet<String>,
    pub pending: HashSet<String>,
    pub consumed_prefix: String,
    pub last_streaming_browser_open_action: Option<String>,
}

// ---------------------------------------------------------------------------
// Voice command feedback
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct AsrStatus {
    pub configured: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct VoiceCommandFeedback {
    pub level: String,
    pub message: String,
}

#[derive(Clone, Debug)]
pub enum ConfigSkillPlan {
    Save {
        config: Box<storage::AppConfig>,
        feedback: VoiceCommandFeedback,
    },
    Feedback(VoiceCommandFeedback),
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

pub const DOUBLE_CLICK_WINDOW_MS: u64 = 280;
pub const INDICATOR_LOGICAL_WIDTH: f64 = 800.0;
pub const INDICATOR_COLLAPSED_HEIGHT: f64 = 200.0;
pub const INDICATOR_EXPANDED_HEIGHT: f64 = 520.0;
pub const INDICATOR_BOTTOM_MARGIN: f64 = 120.0;

// ---------------------------------------------------------------------------
// Utility
// ---------------------------------------------------------------------------

/// Keep logs readable: single-line preview with a hard cap.
pub fn preview_text(s: &str, max_chars: usize) -> String {
    let mut out = String::with_capacity(max_chars.min(s.len()));
    for ch in s.chars() {
        if ch == '\n' || ch == '\r' || ch == '\t' {
            out.push(' ');
        } else {
            out.push(ch);
        }
        if out.chars().count() >= max_chars {
            break;
        }
    }
    out
}
