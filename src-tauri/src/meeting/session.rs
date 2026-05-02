//! Meeting session orchestration.
//!
//! Each meeting is a single ASR streaming session whose partial text is
//! relayed to the frontend via `meeting_partial` events. On stop, the final
//! transcript becomes the meeting's `raw_text`. LLM correction + summary
//! happen in Phase 4.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use anyhow::{anyhow, Result};
use chrono::Utc;
use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::asr::{AsrService, StreamingSession};
use crate::meeting::audio::{MeetingAudioCapture, MeetingAudioConfig};
use crate::state::StorageState;
use crate::storage::{
    MeetingAudioSource, MeetingRecord, MeetingSegment, MeetingStatus, StorageService,
};

const MEETING_DRAFT_SAVE_INTERVAL_MS: u64 = 1_500;

/// Live, in-memory state for the current (single) meeting. Persisted to
/// disk on stop.
pub struct ActiveMeeting {
    pub id: String,
    pub started_at_iso: String,
    pub started_at_instant: std::time::Instant,
    pub asr_provider_name: String,
    pub audio_source: MeetingAudioSource,
    capture: MeetingAudioCapture,
    asr_session: StreamingSession,
    partial_text: Arc<Mutex<String>>,
}

impl ActiveMeeting {
    /// Stop capture, drain ASR, build the persisted record.
    pub fn stop(self) -> Result<MeetingRecord> {
        let ActiveMeeting {
            id,
            started_at_iso,
            started_at_instant,
            asr_provider_name,
            audio_source,
            capture,
            asr_session,
            partial_text,
        } = self;
        let draft_audio_path = capture
            .draft_audio_path()
            .map(|path| path.to_string_lossy().to_string());

        // 1. Drop the audio capture → its sender is dropped → ASR session sees
        //    EOF and can flush.
        capture.stop();

        // 2. Wait for the ASR final transcript.
        let final_text = match asr_session.finish_and_wait() {
            Ok(t) => t,
            Err(e) => {
                // Fall back to whatever partials we accumulated.
                eprintln!("[MEETING] asr finish error: {e}; using partial text");
                partial_text.lock().map(|g| g.clone()).unwrap_or_default()
            }
        };

        let duration_ms = started_at_instant.elapsed().as_millis() as u64;
        let ended_at_iso = Utc::now().to_rfc3339();

        let segments = if final_text.trim().is_empty() {
            Vec::new()
        } else {
            // Single-segment record for now; per-utterance segmentation will
            // come with the trait-level segment events upgrade.
            vec![MeetingSegment {
                start_ms: 0,
                end_ms: duration_ms,
                text: final_text.clone(),
                speaker: None,
            }]
        };

        Ok(MeetingRecord {
            id,
            started_at: started_at_iso,
            ended_at: Some(ended_at_iso),
            duration_ms,
            audio_source,
            asr_provider: asr_provider_name,
            status: MeetingStatus::RawOnly,
            segments,
            raw_text: final_text,
            corrected_text: None,
            summary: None,
            last_error: None,
            draft_audio_path,
        })
    }
}

static MEETING_SEQ: AtomicU64 = AtomicU64::new(1);

fn next_meeting_id() -> String {
    let n = MEETING_SEQ.fetch_add(1, Ordering::Relaxed);
    let ts = Utc::now().format("%Y%m%dT%H%M%S");
    format!("meeting-{ts}-{n:04}")
}

/// Begin a meeting: open audio + ASR streaming session, return the
/// `ActiveMeeting`. Caller stores it in `MeetingState`.
pub fn start_meeting<R: Runtime>(
    app: AppHandle<R>,
    asr: &AsrService,
    storage: &StorageService,
    opts: MeetingAudioConfig,
) -> Result<ActiveMeeting> {
    let config = storage.load_config();
    let device_id = config.input_device.clone();

    let id = next_meeting_id();
    let started_at_iso = Utc::now().to_rfc3339();
    let started_at_instant = std::time::Instant::now();
    let provider_name = asr.current().name().to_string();
    let audio_source = if opts.include_system_audio {
        MeetingAudioSource::MicAndLoopback
    } else {
        MeetingAudioSource::MicOnly
    };
    let draft_audio_path = meeting_draft_audio_path(&app, &id);

    let mut capture = MeetingAudioCapture::start(
        app.clone(),
        &device_id,
        draft_audio_path.clone(),
        opts,
    )?;
    let audio_rx = capture
        .take_audio_rx()
        .ok_or_else(|| anyhow!("audio capture missing receiver"))?;
    let sample_rate = capture.sample_rate();

    let partial_text = Arc::new(Mutex::new(String::new()));
    let partial_for_cb = partial_text.clone();
    let app_for_cb = app.clone();
    let id_for_cb = id.clone();
    let started_at_iso_for_cb = started_at_iso.clone();
    let provider_name_for_cb = provider_name.clone();
    let audio_source_for_cb = audio_source.clone();
    let draft_audio_path_for_cb = draft_audio_path
        .as_ref()
        .map(|path| path.to_string_lossy().to_string());
    let last_draft_save = Arc::new(AtomicU64::new(0));
    let last_draft_save_for_cb = last_draft_save.clone();

    let asr_session = asr.start_streaming_session(audio_rx, sample_rate, move |text| {
        if let Ok(mut guard) = partial_for_cb.lock() {
            *guard = text.clone();
        }
        maybe_persist_meeting_draft(
            &app_for_cb,
            &id_for_cb,
            &started_at_iso_for_cb,
            started_at_instant,
            &provider_name_for_cb,
            audio_source_for_cb.clone(),
            draft_audio_path_for_cb.clone(),
            text.clone(),
            &last_draft_save_for_cb,
        );
        let _ = app_for_cb.emit(
            "meeting_partial",
            MeetingPartialEvent {
                session_id: id_for_cb.clone(),
                text,
            },
        );
    })?;

    // Notify frontend.
    let _ = app.emit(
        "meeting_status",
        MeetingStatusEvent {
            state: "recording",
            session_id: Some(id.clone()),
        },
    );

    let _ = storage.save_meeting(build_meeting_draft_record(
        &id,
        &started_at_iso,
        started_at_instant,
        &provider_name,
        audio_source.clone(),
        draft_audio_path
            .as_ref()
            .map(|path| path.to_string_lossy().to_string()),
        String::new(),
    ));

    // Background watchdog to emit an audio-level proxy (none extra needed —
    // capture already emits `meeting_audio_level`).
    drop(thread::spawn(move || {})); // placeholder, kept for future heartbeat.

    Ok(ActiveMeeting {
        id,
        started_at_iso,
        started_at_instant,
        asr_provider_name: provider_name,
        audio_source,
        capture,
        asr_session,
        partial_text,
    })
}

#[derive(serde::Serialize, Clone)]
pub struct MeetingStatusEvent {
    pub state: &'static str,
    pub session_id: Option<String>,
}

#[derive(serde::Serialize, Clone)]
pub struct MeetingPartialEvent {
    pub session_id: String,
    pub text: String,
}

#[derive(serde::Serialize, Clone)]
pub struct MeetingFinalizedEvent {
    pub id: String,
}

fn build_meeting_draft_record(
    id: &str,
    started_at_iso: &str,
    started_at_instant: std::time::Instant,
    provider_name: &str,
    audio_source: MeetingAudioSource,
    draft_audio_path: Option<String>,
    raw_text: String,
) -> MeetingRecord {
    let duration_ms = started_at_instant.elapsed().as_millis() as u64;
    let segments = if raw_text.trim().is_empty() {
        Vec::new()
    } else {
        vec![MeetingSegment {
            start_ms: 0,
            end_ms: duration_ms,
            text: raw_text.clone(),
            speaker: None,
        }]
    };

    MeetingRecord {
        id: id.to_string(),
        started_at: started_at_iso.to_string(),
        ended_at: None,
        duration_ms,
        audio_source,
        asr_provider: provider_name.to_string(),
        status: MeetingStatus::Recording,
        segments,
        raw_text,
        corrected_text: None,
        summary: None,
        last_error: None,
        draft_audio_path,
    }
}

fn maybe_persist_meeting_draft<R: Runtime>(
    app: &AppHandle<R>,
    id: &str,
    started_at_iso: &str,
    started_at_instant: std::time::Instant,
    provider_name: &str,
    audio_source: MeetingAudioSource,
    draft_audio_path: Option<String>,
    raw_text: String,
    last_saved_at: &AtomicU64,
) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0);
    let previous = last_saved_at.load(Ordering::Relaxed);
    if previous != 0 && now.saturating_sub(previous) < MEETING_DRAFT_SAVE_INTERVAL_MS {
        return;
    }
    last_saved_at.store(now, Ordering::Relaxed);

    let Some(storage) = app.try_state::<StorageState>() else {
        return;
    };

    let _ = storage.save_meeting(build_meeting_draft_record(
        id,
        started_at_iso,
        started_at_instant,
        provider_name,
        audio_source,
        draft_audio_path,
        raw_text,
    ));
}

fn meeting_draft_audio_path<R: Runtime>(app: &AppHandle<R>, id: &str) -> Option<PathBuf> {
    let app_dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| PathBuf::from("data"));
    Some(app_dir.join("meetings").join("drafts").join(format!("{}.wav", id)))
}
