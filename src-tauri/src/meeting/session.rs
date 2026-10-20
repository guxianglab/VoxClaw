//! Meeting session orchestration.
//!
//! A meeting is one streaming session on the currently selected ASR engine
//! (Volcengine or Zipformer), fed by the mic + system-audio mix. Partial text
//! is relayed to the frontend via `meeting_partial` events and periodically
//! persisted as a draft so a crash doesn't lose the transcript. On stop the
//! session is drained and its final text becomes the meeting transcript.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use anyhow::{anyhow, Result};
use chrono::Utc;
use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::asr::{AsrService, StreamingSession};
use crate::meeting::audio::MeetingAudioCapture;
use crate::state::StorageState;
use crate::storage::{MeetingRecord, MeetingStatus, StorageService};

const MEETING_DRAFT_SAVE_INTERVAL_MS: u64 = 5_000;

/// Live, in-memory state for the current (single) meeting.
pub struct ActiveMeeting {
    pub id: String,
    pub started_at: String,
    pub asr_provider: String,
    started_instant: Instant,
    capture: MeetingAudioCapture,
    session: StreamingSession,
}

impl ActiveMeeting {
    fn record(&self, status: MeetingStatus, transcript: String) -> MeetingRecord {
        build_record(
            &self.id,
            &self.started_at,
            self.started_instant,
            &self.asr_provider,
            status,
            transcript,
        )
    }

    /// Stop capture, drain the ASR session and build the persisted record.
    /// Blocking: call from a blocking thread.
    pub fn stop(self) -> Result<MeetingRecord> {
        let ActiveMeeting {
            id,
            started_at,
            asr_provider,
            started_instant,
            capture,
            session,
        } = self;

        // Stop capture → its sender drops → the ASR session sees EOF.
        capture.stop();
        let transcript = session.finish_and_wait()?;

        let mut record = build_record(
            &id,
            &started_at,
            started_instant,
            &asr_provider,
            MeetingStatus::Transcribed,
            transcript,
        );
        record.ended_at = Some(Utc::now().to_rfc3339());
        Ok(record)
    }
}

fn build_record(
    id: &str,
    started_at: &str,
    started_instant: Instant,
    asr_provider: &str,
    status: MeetingStatus,
    transcript: String,
) -> MeetingRecord {
    MeetingRecord {
        id: id.to_string(),
        started_at: started_at.to_string(),
        ended_at: None,
        duration_ms: started_instant.elapsed().as_millis() as u64,
        asr_provider: asr_provider.to_string(),
        status,
        transcript,
        summary: None,
        last_error: None,
    }
}

/// Begin a meeting: open audio capture and start a streaming ASR session.
pub fn start_meeting<R: Runtime>(
    app: AppHandle<R>,
    asr: &AsrService,
    storage: &StorageService,
) -> Result<ActiveMeeting> {
    let config = storage.load_config();
    let id = format!("meeting-{}", Utc::now().format("%Y%m%dT%H%M%S"));
    let started_at = Utc::now().to_rfc3339();
    let started_instant = Instant::now();
    let asr_provider = asr.current().name().to_string();

    let mut capture = MeetingAudioCapture::start(app.clone(), &config.input_device)?;
    let audio_rx = capture
        .take_audio_rx()
        .ok_or_else(|| anyhow!("audio capture missing receiver"))?;

    let last_draft_save = Arc::new(AtomicU64::new(0));
    let (app_cb, id_cb, started_cb, provider_cb) =
        (app.clone(), id.clone(), started_at.clone(), asr_provider.clone());
    let on_update = move |text: String| {
        let _ = app_cb.emit(
            "meeting_partial",
            MeetingPartialEvent {
                session_id: id_cb.clone(),
                text: text.clone(),
            },
        );

        let now = started_instant.elapsed().as_millis() as u64;
        let last = last_draft_save.load(Ordering::Relaxed);
        if now.saturating_sub(last) >= MEETING_DRAFT_SAVE_INTERVAL_MS {
            last_draft_save.store(now, Ordering::Relaxed);
            if let Some(storage) = app_cb.try_state::<StorageState>() {
                let _ = storage.save_meeting(build_record(
                    &id_cb,
                    &started_cb,
                    started_instant,
                    &provider_cb,
                    MeetingStatus::Recording,
                    text,
                ));
            }
        }
    };

    let session = match asr.start_streaming_session(audio_rx, capture.sample_rate(), on_update) {
        Ok(session) => session,
        Err(e) => {
            capture.stop();
            return Err(anyhow!("语音识别启动失败: {e}"));
        }
    };

    let meeting = ActiveMeeting {
        id,
        started_at,
        asr_provider,
        started_instant,
        capture,
        session,
    };
    let _ = storage.save_meeting(meeting.record(MeetingStatus::Recording, String::new()));
    let _ = app.emit(
        "meeting_status",
        MeetingStatusEvent {
            state: "recording",
            session_id: Some(meeting.id.clone()),
        },
    );
    Ok(meeting)
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
