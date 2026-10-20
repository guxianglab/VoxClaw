//! Tauri commands for meeting mode.

use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::meeting::session::{self, MeetingStatusEvent};
use crate::meeting::summary;
use crate::state::{MeetingState, StorageState};
use crate::storage::{MeetingRecord, MeetingStatus, MeetingSummaryItem};

#[derive(serde::Serialize, Clone)]
pub struct MeetingActiveInfo {
    pub session_id: String,
    pub started_at: String,
    pub asr_provider: String,
}

#[derive(serde::Serialize, Clone)]
struct MeetingUpdatedEvent {
    id: String,
    /// True while a summary is being generated for this meeting.
    summarizing: bool,
}

/// Begin a new meeting recording. Fails if one is already running.
#[tauri::command]
pub fn start_meeting<R: Runtime>(
    app: AppHandle<R>,
    state: tauri::State<MeetingState>,
    storage: tauri::State<StorageState>,
    asr: tauri::State<crate::state::AsrState>,
) -> Result<MeetingActiveInfo, String> {
    let mut guard = state.lock().map_err(|_| "meeting state poisoned".to_string())?;
    if guard.is_some() {
        return Err("已有会议正在进行".into());
    }
    let active = session::start_meeting(app, &asr, &storage).map_err(|e| e.to_string())?;
    let info = MeetingActiveInfo {
        session_id: active.id.clone(),
        started_at: active.started_at.clone(),
        asr_provider: active.asr_provider.clone(),
    };
    *guard = Some(active);
    Ok(info)
}

/// Stop the active meeting and persist its transcript. When an LLM is
/// configured, a summary is generated in the background and announced with
/// `meeting_updated` events.
#[tauri::command]
pub async fn stop_meeting<R: Runtime>(app: AppHandle<R>) -> Result<MeetingRecord, String> {
    let active = {
        let state = app.state::<MeetingState>();
        let mut guard = state.lock().map_err(|_| "meeting state poisoned".to_string())?;
        guard.take().ok_or_else(|| "当前没有进行中的会议".to_string())?
    };

    let _ = app.emit(
        "meeting_status",
        MeetingStatusEvent {
            state: "finalizing",
            session_id: Some(active.id.clone()),
        },
    );

    // Draining the ASR session blocks; keep it off the async runtime.
    let result = tokio::task::spawn_blocking(move || active.stop())
        .await
        .map_err(|e| format!("join error: {e}"))?;

    let _ = app.emit(
        "meeting_status",
        MeetingStatusEvent {
            state: "idle",
            session_id: None,
        },
    );

    let record = result.map_err(|e| format!("结束会议失败: {e}"))?;
    let storage = app.state::<StorageState>();
    storage.save_meeting(record.clone()).map_err(|e| e.to_string())?;

    if !record.transcript.trim().is_empty() && storage.load_config().llm_config.is_configured() {
        let app = app.clone();
        let record = record.clone();
        tauri::async_runtime::spawn(async move {
            let _ = summarize(&app, record).await;
        });
    }

    Ok(record)
}

#[tauri::command]
pub fn get_active_meeting(
    state: tauri::State<MeetingState>,
) -> Result<Option<MeetingActiveInfo>, String> {
    let guard = state.lock().map_err(|_| "meeting state poisoned".to_string())?;
    Ok(guard.as_ref().map(|m| MeetingActiveInfo {
        session_id: m.id.clone(),
        started_at: m.started_at.clone(),
        asr_provider: m.asr_provider.clone(),
    }))
}

#[tauri::command]
pub fn list_meetings(storage: tauri::State<StorageState>) -> Vec<MeetingSummaryItem> {
    storage.list_meetings()
}

#[tauri::command]
pub fn get_meeting(storage: tauri::State<StorageState>, id: String) -> Option<MeetingRecord> {
    storage.load_meeting(&id)
}

#[tauri::command]
pub fn delete_meeting(storage: tauri::State<StorageState>, id: String) -> Result<(), String> {
    storage.delete_meeting(id).map_err(|e| e.to_string())
}

/// (Re)generate the LLM summary for a stored meeting.
#[tauri::command]
pub async fn summarize_meeting<R: Runtime>(
    app: AppHandle<R>,
    id: String,
) -> Result<MeetingRecord, String> {
    let record = app
        .state::<StorageState>()
        .load_meeting(&id)
        .ok_or_else(|| format!("meeting {id} not found"))?;
    summarize(&app, record).await
}

async fn summarize<R: Runtime>(
    app: &AppHandle<R>,
    mut record: MeetingRecord,
) -> Result<MeetingRecord, String> {
    let storage = app.state::<StorageState>();
    if record.transcript.trim().is_empty() {
        return Err("会议没有转写内容".into());
    }

    let _ = app.emit(
        "meeting_updated",
        MeetingUpdatedEvent { id: record.id.clone(), summarizing: true },
    );
    let config = storage.load_config();
    match summary::summarise_transcript(&record.transcript, &config.llm_config, &config.proxy).await
    {
        Ok(summary) => {
            record.summary = Some(summary);
            record.status = MeetingStatus::Summarized;
            record.last_error = None;
        }
        Err(e) => {
            record.status = MeetingStatus::Failed;
            record.last_error = Some(format!("生成总结失败: {e}"));
        }
    }

    let _ = storage.save_meeting(record.clone());
    let _ = app.emit(
        "meeting_updated",
        MeetingUpdatedEvent { id: record.id.clone(), summarizing: false },
    );
    match &record.last_error {
        Some(err) => Err(err.clone()),
        None => Ok(record),
    }
}

/// Helper to be called once during setup to register the meeting state.
pub fn init_meeting_state() -> MeetingState {
    Arc::new(Mutex::new(None))
}
