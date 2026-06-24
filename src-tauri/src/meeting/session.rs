//! Meeting session orchestration.
//!
//! Each meeting drives a [`MeetingPipeline`] (resample → VAD → per-segment
//! SenseVoice inference) on a dedicated feeder thread. Partial text is
//! relayed to the frontend via `meeting_partial` events as each segment is
//! transcribed. On stop, the feeder is joined (it flushes the final segment)
//! and the accumulated transcript becomes the meeting's `raw_text`.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use anyhow::{anyhow, Result};
use chrono::Utc;
use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::asr::sensevoice::model as sv_model;
use crate::asr::sensevoice::provider::SenseVoiceProvider;
use crate::asr::sensevoice::vad::{SileroVad, VadEndpointerConfig};
use crate::asr::AsrService;
use crate::meeting::audio::{MeetingAudioCapture, MeetingAudioConfig};
use crate::meeting::pipeline::{self, PipelineResult};
use crate::state::StorageState;
use crate::storage::{
    MeetingAudioSource, MeetingRecord, MeetingSegment, MeetingStatus, StorageService,
};

const MEETING_DRAFT_SAVE_INTERVAL_MS: u64 = 1_500;
const TARGET_SR: f32 = 16_000.0;

/// Live, in-memory state for the current (single) meeting. Persisted to
/// disk on stop.
pub struct ActiveMeeting {
    pub id: String,
    pub started_at_iso: String,
    pub started_at_instant: std::time::Instant,
    pub asr_provider_name: String,
    pub audio_source: MeetingAudioSource,
    capture: MeetingAudioCapture,
    /// Feeder thread running the pipeline; deposits its result on exit.
    feeder: Option<thread::JoinHandle<Option<PipelineResult>>>,
    partial_text: Arc<Mutex<String>>,
}

impl ActiveMeeting {
    /// Stop capture, join the feeder (flushing the final segment), build the
    /// persisted record.
    pub fn stop(mut self) -> Result<MeetingRecord> {
        let ActiveMeeting {
            id,
            started_at_iso,
            started_at_instant,
            asr_provider_name,
            audio_source,
            capture,
            mut feeder,
            partial_text,
        } = self;
        let draft_audio_path = capture
            .draft_audio_path()
            .map(|path| path.to_string_lossy().to_string());

        // 1. Stop capture → its sender drops → pipeline feeder sees EOF.
        capture.stop();

        // 2. Join the feeder thread; it flushes the final segment on EOF.
        let pipeline_result = match feeder.take() {
            Some(h) => h.join().ok().flatten(),
            None => None,
        };

        let (final_text, segments) = match pipeline_result {
            Some(r) => (r.full_text, r.segments),
            None => {
                eprintln!("[MEETING] pipeline produced no result; using partial text");
                let text = partial_text.lock().map(|g| g.clone()).unwrap_or_default();
                (text, Vec::new())
            }
        };

        let duration_ms = started_at_instant.elapsed().as_millis() as u64;
        let ended_at_iso = Utc::now().to_rfc3339();

        let segments = if !segments.is_empty() {
            segments
                .into_iter()
                .map(|s| MeetingSegment {
                    start_ms: s.start_ms,
                    end_ms: s.end_ms,
                    text: s.text,
                    speaker: s.speaker,
                })
                .collect()
        } else if final_text.trim().is_empty() {
            Vec::new()
        } else {
            // Fallback: single segment covering the whole duration.
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

/// Begin a meeting: open audio + build the VAD pipeline, return the
/// `ActiveMeeting`. Caller stores it in `MeetingState`.
pub fn start_meeting<R: Runtime>(
    app: AppHandle<R>,
    asr: &AsrService,
    storage: &StorageService,
    opts: MeetingAudioConfig,
) -> Result<ActiveMeeting> {
    let config = storage.load_config();
    let device_id = config.input_device.clone();

    // Meeting mode requires the SenseVoice offline engine + its VAD model.
    let current = asr.current();
    if current.as_sensevoice().is_none() {
        return Err(anyhow!(
            "会议模式当前仅支持 SenseVoice 离线引擎（当前引擎: {}）。请先在设置中切换到离线引擎。",
            current.name()
        ));
    }
    let sv_dir = std::path::PathBuf::from(&config.asr.sensevoice.model_dir);
    if !sv_model::is_vad_present(&sv_dir) {
        return Err(anyhow!(
            "VAD 模型未找到（{}）。请先在设置中下载 VAD 模型。",
            sv_model::vad_model_file(&sv_dir).display()
        ));
    }

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

    // Build a dedicated SenseVoice provider for the pipeline from config.
    // Independent from the dictation path; cheap relative to a whole meeting.
    let sv_provider =
        SenseVoiceProvider::try_new(&config.asr.sensevoice)
            .map_err(|e| anyhow!("SenseVoice 加载失败: {e}"))?;
    let provider = Arc::new(sv_provider);

    let vad = Arc::new(
        SileroVad::try_new(&sv_model::vad_model_file(&sv_dir), config.asr.sensevoice.use_gpu)
            .map_err(|e| anyhow!("VAD 模型加载失败: {e}"))?,
    );

    let vad_cfg = VadEndpointerConfig {
        threshold: if config.asr.sensevoice.vad_threshold > 0.0 {
            config.asr.sensevoice.vad_threshold
        } else {
            VadEndpointerConfig::default().threshold
        },
        min_silence_samples: ms_to_samples(vad_min_silence_ms(&config)),
        ..VadEndpointerConfig::default()
    };

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

    let provider_for_thread = provider.clone();
    let vad_for_thread = vad.clone();
    let feeder = thread::Builder::new()
        .name("meeting-pipeline".into())
        .spawn(move || {
            let result = pipeline::run_pipeline(
                audio_rx,
                sample_rate,
                vad_for_thread,
                provider_for_thread,
                vad_cfg,
                move |full_text: &str| {
                    if let Ok(mut guard) = partial_for_cb.lock() {
                        *guard = full_text.to_string();
                    }
                    maybe_persist_meeting_draft(
                        &app_for_cb,
                        &id_for_cb,
                        &started_at_iso_for_cb,
                        started_at_instant,
                        &provider_name_for_cb,
                        audio_source_for_cb.clone(),
                        draft_audio_path_for_cb.clone(),
                        full_text.to_string(),
                        &last_draft_save_for_cb,
                    );
                    let _ = app_for_cb.emit(
                        "meeting_partial",
                        MeetingPartialEvent {
                            session_id: id_for_cb.clone(),
                            text: full_text.to_string(),
                        },
                    );
                },
            );
            match result {
                Ok(r) => Some(r),
                Err(e) => {
                    eprintln!("[MEETING] pipeline error: {e}");
                    None
                }
            }
        })
        .map_err(|e| anyhow!("failed to spawn pipeline thread: {e}"))?;

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

    Ok(ActiveMeeting {
        id,
        started_at_iso,
        started_at_instant,
        asr_provider_name: provider_name,
        audio_source,
        capture,
        feeder: Some(feeder),
        partial_text,
    })
}

fn vad_min_silence_ms(config: &crate::storage::AppConfig) -> u32 {
    let v = config.asr.sensevoice.vad_min_silence_ms;
    if v > 0 {
        v
    } else {
        500
    }
}

fn ms_to_samples(ms: u32) -> u64 {
    (ms as f64 * TARGET_SR as f64 / 1000.0) as u64
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
