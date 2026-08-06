//! Streaming Paraformer ASR provider.
//!
//! Structurally identical to [`crate::asr::zipformer::ZipformerProvider`] —
//! both wrap `sherpa_onnx::OnlineRecognizer`. The only difference is the model
//! config branch: `OnlineParaformerModelConfig` (encoder + decoder) instead of
//! the transducer triad. The feeder-thread loop
//! (`accept_waveform → decode → get_result → is_endpoint/reset`) is byte-for-byte
//! the same, because Paraformer streaming and Zipformer streaming share the
//! same `OnlineRecognizer` contract.

use std::path::PathBuf;
use std::sync::Arc;
use std::thread;

use anyhow::{anyhow, Result};
use sherpa_onnx::{
    OnlineModelConfig, OnlineParaformerModelConfig, OnlineRecognizer, OnlineRecognizerConfig,
};

use crate::asr::{
    AsrCapabilities, AsrProvider, AsrSession, AsrStreamParams, TranscriptSegment,
};
use crate::asr::paraformer::model;
use crate::asr::resampler::StreamingResampler16k;
use crate::storage::ParaformerStreamingConfig;

const TARGET_SR: i32 = 16_000;

/// sherpa-onnx Streaming Paraformer provider. Holds one `OnlineRecognizer`
/// (thread-safe, reused across sessions).
pub struct ParaformerProvider {
    recognizer: Arc<OnlineRecognizer>,
}

impl ParaformerProvider {
    pub fn try_new(config: &ParaformerStreamingConfig) -> Result<Self> {
        let dir = if config.model_dir.is_empty() {
            return Err(anyhow!(
                "Paraformer 模型目录未设置。请在设置中下载 Paraformer 流式模型。"
            ));
        } else {
            PathBuf::from(&config.model_dir)
        };

        if !model::is_present(&dir) {
            let missing = model::missing_files(&dir).join(", ");
            return Err(anyhow!(
                "Paraformer 模型文件不完整，缺少: {missing}。请在设置中下载模型。"
            ));
        }

        let encoder = model::encoder_file(&dir)
            .ok_or_else(|| anyhow!("encoder .onnx not found"))?;
        let decoder = model::decoder_file(&dir)
            .ok_or_else(|| anyhow!("decoder .onnx not found"))?;
        let tokens = model::tokens_file(&dir);

        let mut model_config = OnlineModelConfig::default();
        model_config.paraformer = OnlineParaformerModelConfig {
            encoder: Some(encoder.to_string_lossy().into_owned()),
            decoder: Some(decoder.to_string_lossy().into_owned()),
        };
        model_config.tokens = Some(tokens.to_string_lossy().into_owned());
        model_config.num_threads = paraformer_num_threads();

        let mut cfg = OnlineRecognizerConfig::default();
        cfg.model_config = model_config;
        cfg.enable_endpoint = config.enable_endpoint;
        cfg.decoding_method = Some("greedy_search".to_string());
        // Endpoint defaults tuned for continuous speech (meetings):
        //   rule1: 2.4s trailing silence → end utterance (paragraph break).
        //   rule2: 1.2s trailing silence after >=20s of speech → end.
        //   rule3: hard cap utterance at 20s.
        cfg.rule1_min_trailing_silence = 2.4;
        cfg.rule2_min_trailing_silence = 1.2;
        cfg.rule3_min_utterance_length = 20.0;

        let recognizer = OnlineRecognizer::create(&cfg)
            .ok_or_else(|| anyhow!("创建 Paraformer OnlineRecognizer 失败"))?;

        println!("[PARAFORMER] model loaded from {}", dir.display());

        Ok(Self {
            recognizer: Arc::new(recognizer),
        })
    }
}

impl AsrProvider for ParaformerProvider {
    fn name(&self) -> &'static str {
        "paraformer_streaming"
    }

    fn capabilities(&self) -> AsrCapabilities {
        AsrCapabilities {
            streaming: true,
            offline: true,
            languages: vec!["zh".into(), "en".into()],
            supports_diarization: false,
        }
    }

    fn start_streaming(&self, params: AsrStreamParams) -> Result<Box<dyn AsrSession>> {
        let AsrStreamParams {
            audio_rx,
            sample_rate,
            on_update,
        } = params;

        let recognizer = self.recognizer.clone();
        let (handle, collected) =
            ParaformerSession::spawn(recognizer, audio_rx, sample_rate, on_update)?;
        Ok(Box::new(ParaformerSession {
            handle: Some(handle),
            collected,
        }))
    }
}

/// A streaming session: a feeder thread pumps audio, accumulates final text,
/// and emits partials. `stop()` joins the thread and returns the full text.
pub struct ParaformerSession {
    handle: Option<thread::JoinHandle<(String, Vec<TranscriptSegment>)>>,
    /// Shared accumulator, readable before the thread joins (for live partials).
    collected: Arc<std::sync::Mutex<Collected>>,
}

struct Collected {
    full_text: String,
    segments: Vec<TranscriptSegment>,
}

impl ParaformerSession {
    fn spawn(
        recognizer: Arc<OnlineRecognizer>,
        audio_rx: std::sync::mpsc::Receiver<Vec<f32>>,
        sample_rate: u32,
        on_update: Box<dyn Fn(String) + Send + Sync + 'static>,
    ) -> Result<(
        thread::JoinHandle<(String, Vec<TranscriptSegment>)>,
        Arc<std::sync::Mutex<Collected>>,
    )> {
        let collected = Arc::new(std::sync::Mutex::new(Collected {
            full_text: String::new(),
            segments: Vec::new(),
        }));

        let needs_resample = (sample_rate as i32 - TARGET_SR).abs() >= 1;
        let mut resampler = if needs_resample {
            Some(StreamingResampler16k::new(sample_rate)?)
        } else {
            None
        };

        let collected_for_thread = collected.clone();
        let handle = thread::Builder::new()
            .name("paraformer-stream".into())
            .spawn(move || {
                let stream = recognizer.create_stream();
                let mut utterance_start_ms: u64 = 0;
                let mut total_ms: u64 = 0;
                let mut last_partial = String::new();

                while let Ok(chunk) = audio_rx.recv() {
                    // Resample to 16k if needed.
                    let mono_16k = match resampler.as_mut() {
                        Some(r) => r.process(&chunk),
                        None => chunk,
                    };
                    if mono_16k.is_empty() {
                        continue;
                    }

                    stream.accept_waveform(TARGET_SR, &mono_16k);

                    // Decode as much as possible.
                    while recognizer.is_ready(&stream) {
                        recognizer.decode(&stream);
                    }

                    // Emit partial result.
                    if let Some(result) = recognizer.get_result(&stream) {
                        let partial = result.text.trim().to_string();
                        if partial != last_partial && !partial.is_empty() {
                            last_partial = partial.clone();
                            // Full text so far = finalized utterances + current partial.
                            let snapshot = if let Ok(c) = collected_for_thread.lock() {
                                format!("{}{}", c.full_text, partial)
                            } else {
                                partial.clone()
                            };
                            on_update(snapshot);
                        }
                    }

                    // Endpoint detection: finalize the current utterance.
                    if recognizer.is_endpoint(&stream) {
                        if let Some(result) = recognizer.get_result(&stream) {
                            let text = result.text.trim().to_string();
                            if !text.is_empty() {
                                let end_ms = total_ms;
                                if let Ok(mut c) = collected_for_thread.lock() {
                                    if !c.full_text.is_empty() {
                                        c.full_text.push(' ');
                                    }
                                    c.full_text.push_str(&text);
                                    c.segments.push(TranscriptSegment {
                                        start_ms: utterance_start_ms,
                                        end_ms,
                                        speaker: None,
                                        text: text.clone(),
                                    });
                                }
                            }
                        }
                        last_partial.clear();
                        recognizer.reset(&stream);
                        utterance_start_ms = total_ms;
                    }

                    // Advance the clock by the chunk duration (16k domain).
                    total_ms += (mono_16k.len() as f64 / TARGET_SR as f64 * 1000.0) as u64;
                }

                // EOF: feed the resampler's carry-over tail, then flush any
                // remaining partial as a final utterance.
                if let Some(r) = resampler.as_mut() {
                    let tail = r.flush();
                    if !tail.is_empty() {
                        stream.accept_waveform(TARGET_SR, &tail);
                        while recognizer.is_ready(&stream) {
                            recognizer.decode(&stream);
                        }
                    }
                }
                stream.input_finished();
                while recognizer.is_ready(&stream) {
                    recognizer.decode(&stream);
                }
                if let Some(result) = recognizer.get_result(&stream) {
                    let text = result.text.trim().to_string();
                    if !text.is_empty() {
                        if let Ok(mut c) = collected_for_thread.lock() {
                            if !c.full_text.is_empty() {
                                c.full_text.push(' ');
                            }
                            c.full_text.push_str(&text);
                            c.segments.push(TranscriptSegment {
                                start_ms: utterance_start_ms,
                                end_ms: total_ms,
                                speaker: None,
                                text,
                            });
                        }
                    }
                }

                let c = match collected_for_thread.lock() {
                    Ok(c) => (c.full_text.clone(), c.segments.clone()),
                    Err(poisoned) => {
                        let c = poisoned.into_inner();
                        (c.full_text.clone(), c.segments.clone())
                    }
                };
                c
            })
            .map_err(|e| anyhow!("failed to spawn paraformer thread: {e}"))?;

        Ok((handle, collected))
    }
}

impl AsrSession for ParaformerSession {
    fn finish_and_wait(mut self: Box<Self>) -> Result<String> {
        // Dropping audio_rx sender (done by caller) causes recv() to return
        // Err, ending the feeder loop. Join to collect the final text.
        if let Some(handle) = self.handle.take() {
            let (text, _) = handle
                .join()
                .map_err(|_| anyhow!("paraformer feeder thread panicked"))?;
            Ok(text)
        } else {
            Err(anyhow!("paraformer session already finished"))
        }
    }

    fn take_segments(&self) -> Vec<TranscriptSegment> {
        self.collected
            .lock()
            .map(|c| c.segments.clone())
            .unwrap_or_default()
    }
}

fn paraformer_num_threads() -> i32 {
    std::thread::available_parallelism()
        .map(|n| (n.get() / 2).max(1) as i32)
        .unwrap_or(2)
}
