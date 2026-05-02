//! Tauri commands for meeting mode.

use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::meeting::audio::MeetingAudioConfig;
use crate::meeting::llm as meeting_llm;
use crate::meeting::session::{self, MeetingFinalizedEvent, MeetingStatusEvent};
use crate::state::{MeetingState, StorageState};
use crate::storage::{HistoryItem, MeetingRecord, MeetingStatus, MeetingSummaryItem};

#[derive(serde::Serialize, Clone)]
pub struct MeetingActiveInfo {
    pub session_id: String,
    pub started_at: String,
    pub asr_provider: String,
    pub include_system_audio: bool,
}

/// Begin a new meeting recording. Fails if one is already running.
#[tauri::command]
pub fn start_meeting<R: Runtime>(
    app: AppHandle<R>,
    state: tauri::State<MeetingState>,
    storage: tauri::State<StorageState>,
    asr: tauri::State<crate::state::AsrState>,
    include_system_audio: Option<bool>,
) -> Result<MeetingActiveInfo, String> {
    let mut guard = state.lock().map_err(|_| "meeting state poisoned".to_string())?;
    if guard.is_some() {
        return Err("a meeting is already in progress".into());
    }
    let opts = MeetingAudioConfig {
        include_system_audio: include_system_audio.unwrap_or(false),
    };
    let active = session::start_meeting(app, &asr, &storage, opts)
        .map_err(|e| format!("start meeting failed: {e}"))?;
    let info = MeetingActiveInfo {
        session_id: active.id.clone(),
        started_at: active.started_at_iso.clone(),
        asr_provider: active.asr_provider_name.clone(),
        include_system_audio: opts.include_system_audio,
    };
    *guard = Some(active);
    Ok(info)
}

/// Stop the active meeting, persist its `MeetingRecord`, return the record.
#[tauri::command]
pub async fn stop_meeting<R: Runtime>(
    app: AppHandle<R>,
) -> Result<MeetingRecord, String> {
    // Take the active meeting out of state on the current thread.
    let active = {
        let state = app
            .try_state::<MeetingState>()
            .ok_or_else(|| "meeting state not initialised".to_string())?;
        let mut guard = state.lock().map_err(|_| "meeting state poisoned".to_string())?;
        guard.take().ok_or_else(|| "no meeting in progress".to_string())?
    };

    let _ = app.emit(
        "meeting_status",
        MeetingStatusEvent {
            state: "finalizing",
            session_id: Some(active.id.clone()),
        },
    );

    // Drain ASR off the runtime thread (finish_and_wait is blocking).
    let record = tokio::task::spawn_blocking(move || active.stop())
        .await
        .map_err(|e| format!("join error: {e}"))?
        .map_err(|e| format!("stop meeting failed: {e}"))?;

    // Persist (async via storage thread).
    if let Some(storage) = app.try_state::<StorageState>() {
        storage
            .save_meeting(record.clone())
            .map_err(|e| format!("save meeting failed: {e}"))?;
    }

    let _ = app.emit(
        "meeting_status",
        MeetingStatusEvent {
            state: "idle",
            session_id: None,
        },
    );
    let _ = app.emit(
        "meeting_finalized",
        MeetingFinalizedEvent { id: record.id.clone() },
    );

    Ok(record)
}

#[tauri::command]
pub fn get_active_meeting(
    state: tauri::State<MeetingState>,
) -> Result<Option<MeetingActiveInfo>, String> {
    let guard = state.lock().map_err(|_| "meeting state poisoned".to_string())?;
    Ok(guard.as_ref().map(|m| MeetingActiveInfo {
        session_id: m.id.clone(),
        started_at: m.started_at_iso.clone(),
        asr_provider: m.asr_provider_name.clone(),
        include_system_audio: matches!(
            m.audio_source,
            crate::storage::MeetingAudioSource::MicAndLoopback
                | crate::storage::MeetingAudioSource::LoopbackOnly
        ),
    }))
}

#[tauri::command]
pub fn list_meetings(
    storage: tauri::State<StorageState>,
) -> Result<Vec<MeetingSummaryItem>, String> {
    Ok(storage.list_meetings())
}

#[tauri::command]
pub fn get_meeting(
    storage: tauri::State<StorageState>,
    id: String,
) -> Result<Option<MeetingRecord>, String> {
    Ok(storage.load_meeting(&id))
}

#[tauri::command]
pub fn delete_meeting(
    storage: tauri::State<StorageState>,
    id: String,
) -> Result<(), String> {
    storage.delete_meeting(id).map_err(|e| e.to_string())
}

/// Helper to be called once during setup to register the meeting state.
pub fn init_meeting_state() -> MeetingState {
    Arc::new(Mutex::new(None))
}

#[derive(serde::Serialize, Clone)]
struct MeetingLlmProgressEvent<'a> {
    id: &'a str,
    /// "correcting" | "correction_done" | "summarising" | "done" | "failed"
    stage: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// Run LLM correction + summarisation for a finalised meeting. The meeting
/// must already exist in storage (i.e. produced by `stop_meeting`).
#[tauri::command]
pub async fn polish_meeting<R: Runtime>(
    app: AppHandle<R>,
    id: String,
) -> Result<MeetingRecord, String> {
    let storage = app
        .try_state::<StorageState>()
        .ok_or_else(|| "storage not initialised".to_string())?;

    let mut record = storage
        .load_meeting(&id)
        .ok_or_else(|| format!("meeting {id} not found"))?;

    if record.raw_text.trim().is_empty() {
        return Err("meeting has no transcript to polish".into());
    }

    let config = storage.load_config();
    let agent_config = config.agent_config.clone();
    let llm_config = config.llm_config.clone();
    let proxy = config.proxy.clone();

    record.status = MeetingStatus::Finalizing;
    record.last_error = None;
    let _ = storage.save_meeting(record.clone());
    let _ = app.emit(
        "meeting_llm_progress",
        MeetingLlmProgressEvent { id: &id, stage: "correcting", error: None },
    );

    let corrected = match meeting_llm::correct_transcript(
        &record.raw_text,
        &agent_config,
        &llm_config,
        &proxy,
    )
    .await
    {
        Ok(t) => t,
        Err(e) => {
            record.status = MeetingStatus::Failed;
            record.last_error = Some(format!("correction failed: {e}"));
            let _ = storage.save_meeting(record.clone());
            let _ = app.emit(
                "meeting_llm_progress",
                MeetingLlmProgressEvent {
                    id: &id,
                    stage: "failed",
                    error: Some(record.last_error.clone().unwrap_or_default()),
                },
            );
            return Err(record.last_error.unwrap_or_default());
        }
    };

    record.corrected_text = Some(corrected.clone());
    record.status = MeetingStatus::Corrected;
    let _ = storage.save_meeting(record.clone());
    let _ = app.emit(
        "meeting_llm_progress",
        MeetingLlmProgressEvent { id: &id, stage: "correction_done", error: None },
    );

    let _ = app.emit(
        "meeting_llm_progress",
        MeetingLlmProgressEvent { id: &id, stage: "summarising", error: None },
    );

    let summary = match meeting_llm::summarise_transcript(
        &corrected,
        &agent_config,
        &llm_config,
        &proxy,
    )
    .await
    {
        Ok(s) => s,
        Err(e) => {
            // Keep corrected text but mark failed for summary stage.
            record.status = MeetingStatus::Failed;
            record.last_error = Some(format!("summary failed: {e}"));
            let _ = storage.save_meeting(record.clone());
            let _ = app.emit(
                "meeting_llm_progress",
                MeetingLlmProgressEvent {
                    id: &id,
                    stage: "failed",
                    error: Some(record.last_error.clone().unwrap_or_default()),
                },
            );
            return Err(record.last_error.unwrap_or_default());
        }
    };

    // Append summary title to the dictation history list so it shows up in
    // the existing "history" view as well.
    let history_label = format!(
        "[会议] {} — {}",
        format_short_date(&record.started_at),
        if summary.title.trim().is_empty() {
            "未命名会议".to_string()
        } else {
            summary.title.clone()
        }
    );
    let _ = storage.add_history_item(HistoryItem {
        id: format!("history-meeting-{}", record.id),
        timestamp: record.started_at.clone(),
        text: history_label,
        duration_ms: record.duration_ms,
    });

    record.summary = Some(summary);
    record.status = MeetingStatus::Summarized;
    record.last_error = None;
    let _ = storage.save_meeting(record.clone());

    let _ = app.emit(
        "meeting_llm_progress",
        MeetingLlmProgressEvent { id: &id, stage: "done", error: None },
    );

    Ok(record)
}

/// Best-effort short date — strips the time portion of an ISO 8601 string.
fn format_short_date(iso: &str) -> String {
    iso.split('T').next().unwrap_or(iso).to_string()
}
