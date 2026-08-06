//! ASR provider abstraction.
//!
//! `AsrProvider` is the per-implementation trait. `AsrService` is the
//! Tauri-managed façade that holds the currently active provider and exposes
//! a stable streaming entry point used by `dictation` (and later `meeting`).
//!
//! The original Volcengine bigmodel implementation lives in
//! `volcengine.rs`. New providers (e.g. SenseVoice ONNX) should add a sibling
//! module and be wired into [`build_provider`].

use anyhow::{anyhow, Result};
use std::error::Error;
use std::fmt;
use std::path::Path;
use std::sync::{Arc, Mutex, RwLock};

use crate::storage::{
    AsrConfig, AsrProviderKind, ProxyConfig, SenseVoiceOnnxConfig, ZipformerStreamingConfig,
};

pub mod volcengine;
pub mod sensevoice;
pub mod zipformer;
pub mod resampler;

// ---------------------------------------------------------------------------
// Trait surface
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default)]
pub struct AsrCapabilities {
    pub streaming: bool,
    pub offline: bool,
    pub languages: Vec<String>,
    pub supports_diarization: bool,
}

#[derive(Clone, Debug)]
#[allow(dead_code)] // populated by future SenseVoice / meeting-mode paths
pub struct TranscriptSegment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub speaker: Option<String>,
    pub text: String,
}

pub struct AsrStreamParams {
    pub audio_rx: std::sync::mpsc::Receiver<Vec<f32>>,
    pub sample_rate: u32,
    pub on_update: Box<dyn Fn(String) + Send + Sync + 'static>,
}

pub trait AsrProvider: Send + Sync {
    fn name(&self) -> &'static str;
    fn capabilities(&self) -> AsrCapabilities;
    fn start_streaming(&self, params: AsrStreamParams) -> Result<Box<dyn AsrSession>>;

    /// Downcast hook for meeting mode, which needs the concrete SenseVoice
    /// provider's per-segment inference. Default returns `None`; SenseVoice
    /// overrides to return `Some(self)`.
    fn as_sensevoice(&self) -> Option<&crate::asr::sensevoice::SenseVoiceProvider> {
        None
    }
}

pub trait AsrSession: Send {
    /// Drain any pending audio and return the final transcript text.
    /// Implementations may also expose richer segment data via
    /// [`AsrSession::take_segments`].
    fn finish_and_wait(self: Box<Self>) -> Result<String>;

    /// Return segment-level transcript data (utterances with timestamps).
    /// Default returns empty — providers that support utterance segmentation
    /// should override.
    fn take_segments(&self) -> Vec<TranscriptSegment> {
        Vec::new()
    }
}

// ---------------------------------------------------------------------------
// Factory
// ---------------------------------------------------------------------------

/// Why building a provider failed. `ModelMissing` is *recoverable*: the user
/// simply hasn't downloaded the model yet. Callers (e.g. `save_config`) should
/// persist the config anyway — the engine choice is remembered — and keep the
/// previous provider running until the model download finishes, instead of
/// treating it as a hard save failure.
#[derive(Debug)]
pub enum ProviderBuildError {
    /// A local engine was selected but its model is not downloaded yet.
    ModelMissing(String),
    /// Any other failure (ONNX Runtime, corrupted files, etc.).
    Other(String),
}

impl fmt::Display for ProviderBuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProviderBuildError::ModelMissing(msg) | ProviderBuildError::Other(msg) => {
                f.write_str(msg)
            }
        }
    }
}

impl Error for ProviderBuildError {}

/// True when the SenseVoice model dir is configured and complete.
fn sensevoice_model_ready(config: &SenseVoiceOnnxConfig) -> bool {
    !config.model_dir.is_empty()
        && sensevoice::model::is_present(Path::new(&config.model_dir))
}

/// True when the Zipformer model dir is configured and complete.
fn zipformer_model_ready(config: &ZipformerStreamingConfig) -> bool {
    !config.model_dir.is_empty() && zipformer::model::is_present(Path::new(&config.model_dir))
}

/// Short notice for the "engine selected but model not downloaded" case.
fn model_missing_notice(engine: &str) -> String {
    format!("{engine} 模型未下载，设置已保存。请在下方点击“下载模型”，下载完成后即可使用该引擎。")
}

pub fn build_provider(
    config: &AsrConfig,
    proxy: &ProxyConfig,
) -> Result<Arc<dyn AsrProvider>, ProviderBuildError> {
    match config.provider {
        AsrProviderKind::Volcengine => Ok(Arc::new(volcengine::VolcengineProvider::new(
            config.volcengine.clone(),
            proxy.clone(),
        ))),
        AsrProviderKind::SenseVoiceOnnx => {
            if !sensevoice_model_ready(&config.sensevoice) {
                return Err(ProviderBuildError::ModelMissing(model_missing_notice(
                    "SenseVoice",
                )));
            }
            let provider = sensevoice::SenseVoiceProvider::try_new(&config.sensevoice).map_err(
                |err| {
                    ProviderBuildError::Other(anyhow!(
                        "SenseVoice 离线引擎加载失败: {err}。请检查模型文件是否存在，以及 onnxruntime.dll 是否可用。"
                    )
                    .to_string())
                },
            )?;
            Ok(Arc::new(provider))
        }
        AsrProviderKind::ZipformerStreaming => {
            if !zipformer_model_ready(&config.zipformer) {
                return Err(ProviderBuildError::ModelMissing(model_missing_notice(
                    "Zipformer",
                )));
            }
            let provider = zipformer::ZipformerProvider::try_new(&config.zipformer).map_err(
                |err| {
                    ProviderBuildError::Other(anyhow!(
                        "Zipformer 流式引擎加载失败: {err}。请检查模型文件是否存在。"
                    )
                    .to_string())
                },
            )?;
            Ok(Arc::new(provider))
        }
    }
}

// ---------------------------------------------------------------------------
// Service façade (Tauri-managed)
// ---------------------------------------------------------------------------

/// Holds the currently active ASR provider behind an `RwLock` so that
/// `save_config` can swap it out at runtime when the user changes provider /
/// credentials without tearing down the rest of the app state.
pub struct AsrService {
    inner: Arc<RwLock<Arc<dyn AsrProvider>>>,
}

impl AsrService {
    pub fn new(provider: Arc<dyn AsrProvider>) -> Self {
        Self {
            inner: Arc::new(RwLock::new(provider)),
        }
    }

    /// Replace the active provider. Returns an error if the lock stays
    /// contended (an ASR session — dictation or meeting — is still running),
    /// so callers can surface it to the user instead of silently keeping the
    /// old provider. This is what made "switch engine" appear to no-op.
    pub fn replace(&self, provider: Arc<dyn AsrProvider>) -> Result<()> {
        let name = provider.name();
        // Retry a few times if the lock is contended by an active session.
        for attempt in 1..=5 {
            match self.inner.try_write() {
                Ok(mut guard) => {
                    let old = guard.name();
                    *guard = provider;
                    println!("[ASR] Provider replaced: {} -> {} (attempt {})", old, name, attempt);
                    return Ok(());
                }
                Err(std::sync::TryLockError::WouldBlock) => {
                    if attempt < 5 {
                        eprintln!("[ASR] Provider replace attempt {} blocked (active session?), retrying...", attempt);
                        std::thread::sleep(std::time::Duration::from_millis(200));
                    }
                }
                Err(std::sync::TryLockError::Poisoned(e)) => {
                    return Err(anyhow!("ASR 服务锁损坏，无法切换引擎: {}", e));
                }
            }
        }
        Err(anyhow!(
            "无法切换引擎：当前仍有录音/会议会话占用 ASR 服务（重试 5 次均被阻塞）。请先停止录音或结束会议，再切换引擎。"
        ))
    }

    pub fn current(&self) -> Arc<dyn AsrProvider> {
        self.inner
            .read()
            .map(|g| g.clone())
            .expect("AsrService inner lock poisoned")
    }

    /// Convenience entry-point that mirrors the previous `start_streaming_session`
    /// signature so existing call sites only need a small touch-up.
    pub fn start_streaming_session<F>(
        &self,
        audio_rx: std::sync::mpsc::Receiver<Vec<f32>>,
        sample_rate: u32,
        on_update: F,
    ) -> Result<StreamingSession>
    where
        F: Fn(String) + Send + Sync + 'static,
    {
        let provider = self.current();
        let session = provider.start_streaming(AsrStreamParams {
            audio_rx,
            sample_rate,
            on_update: Box::new(on_update),
        })?;
        Ok(StreamingSession {
            inner: Some(session),
            segments: Arc::new(Mutex::new(Vec::new())),
        })
    }
}

/// Public handle returned to callers. Wraps the provider-specific session so
/// downstream code never depends on a concrete type.
pub struct StreamingSession {
    inner: Option<Box<dyn AsrSession>>,
    segments: Arc<Mutex<Vec<TranscriptSegment>>>,
}

impl StreamingSession {
    pub fn finish_and_wait(mut self) -> Result<String> {
        // Grab segments before consuming the session.
        if let Some(session) = self.inner.as_ref() {
            if let Ok(mut guard) = self.segments.lock() {
                *guard = session.take_segments();
            }
        }
        match self.inner.take() {
            Some(session) => session.finish_and_wait(),
            None => Err(anyhow!("Session already finished")),
        }
    }

    pub fn take_segments(&self) -> Vec<TranscriptSegment> {
        self.segments.lock().map(|g| g.clone()).unwrap_or_default()
    }
}
