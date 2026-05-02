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
use std::sync::{Arc, RwLock};

use crate::storage::{AsrConfig, AsrProviderKind, ProxyConfig};

pub mod volcengine;
pub mod sensevoice;

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
}

pub trait AsrSession: Send {
    /// Drain any pending audio and return the final transcript text.
    /// Implementations may also expose richer segment data via
    /// [`AsrSession::take_segments`].
    fn finish_and_wait(self: Box<Self>) -> Result<String>;
}

// ---------------------------------------------------------------------------
// Factory
// ---------------------------------------------------------------------------

pub fn build_provider(config: &AsrConfig, proxy: &ProxyConfig) -> Arc<dyn AsrProvider> {
    match config.provider {
        AsrProviderKind::Volcengine => Arc::new(volcengine::VolcengineProvider::new(
            config.volcengine.clone(),
            proxy.clone(),
        )),
        AsrProviderKind::SenseVoiceOnnx => {
            match sensevoice::SenseVoiceProvider::try_new(&config.sensevoice) {
                Ok(p) => Arc::new(p),
                Err(err) => {
                    eprintln!(
                        "[ASR] SenseVoice unavailable ({err}); falling back to Volcengine"
                    );
                    Arc::new(volcengine::VolcengineProvider::new(
                        config.volcengine.clone(),
                        proxy.clone(),
                    ))
                }
            }
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

    pub fn replace(&self, provider: Arc<dyn AsrProvider>) {
        if let Ok(mut guard) = self.inner.write() {
            *guard = provider;
        }
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
        })
    }
}

/// Public handle returned to callers. Wraps the provider-specific session so
/// downstream code never depends on a concrete type.
pub struct StreamingSession {
    inner: Option<Box<dyn AsrSession>>,
}

impl StreamingSession {
    pub fn finish_and_wait(mut self) -> Result<String> {
        match self.inner.take() {
            Some(session) => session.finish_and_wait(),
            None => Err(anyhow!("Session already finished")),
        }
    }
}
