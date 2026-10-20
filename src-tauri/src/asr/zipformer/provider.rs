//! Streaming Zipformer ASR provider.
//!
//! Wraps `sherpa_onnx::OnlineRecognizer`. A single recognizer is shared; each
//! streaming session creates its own `OnlineStream` and runs a feeder thread
//! that pumps audio chunks through `accept_waveform` → `decode` →
//! `get_result`, emitting partial text via the `on_update` callback. The
//! recognizer's built-in endpoint detector signals sentence boundaries.

use std::path::PathBuf;
use std::sync::Arc;
use std::thread;

use anyhow::{anyhow, Result};
use sherpa_onnx::{OnlineRecognizer, OnlineRecognizerConfig};

use crate::asr::{AsrProvider, AsrSession, AsrStreamParams};
use crate::asr::resampler::StreamingResampler16k;
use crate::asr::zipformer::model;
use crate::storage::ZipformerStreamingConfig;

const TARGET_SR: i32 = 16_000;

/// sherpa-onnx Streaming Zipformer provider. Holds one `OnlineRecognizer`
/// (thread-safe, reused across sessions).
pub struct ZipformerProvider {
    recognizer: Arc<OnlineRecognizer>,
}

impl ZipformerProvider {
    pub fn try_new(config: &ZipformerStreamingConfig) -> Result<Self> {
        let dir = if config.model_dir.is_empty() {
            return Err(anyhow!(
                "Zipformer 模型目录未设置。请在设置中下载 Zipformer 流式模型。"
            ));
        } else {
            PathBuf::from(&config.model_dir)
        };

        if !model::is_present(&dir) {
            let missing = model::missing_files(&dir).join(", ");
            return Err(anyhow!(
                "Zipformer 模型文件不完整，缺少: {missing}。请在设置中下载模型。"
            ));
        }

        let encoder = model::encoder_file(&dir)
            .ok_or_else(|| anyhow!("encoder .onnx not found"))?;
        let decoder = model::decoder_file(&dir)
            .ok_or_else(|| anyhow!("decoder .onnx not found"))?;
        let joiner = model::joiner_file(&dir)
            .ok_or_else(|| anyhow!("joiner .onnx not found"))?;
        let tokens = model::tokens_file(&dir);

        let mut cfg = OnlineRecognizerConfig::default();
        cfg.model_config.transducer.encoder = Some(encoder.to_string_lossy().to_string());
        cfg.model_config.transducer.decoder = Some(decoder.to_string_lossy().to_string());
        cfg.model_config.transducer.joiner = Some(joiner.to_string_lossy().to_string());
        cfg.model_config.tokens = Some(tokens.to_string_lossy().to_string());
        cfg.enable_endpoint = true;
        cfg.decoding_method = Some("greedy_search".to_string());
        // Endpoint defaults tuned for continuous speech (meetings):
        //   rule1: 2.4s trailing silence → end utterance (paragraph break).
        //   rule2: 1.2s trailing silence after >=20s of speech → end.
        //   rule3: hard cap utterance at 20s.
        cfg.rule1_min_trailing_silence = 2.4;
        cfg.rule2_min_trailing_silence = 1.2;
        cfg.rule3_min_utterance_length = 20.0;

        let recognizer = OnlineRecognizer::create(&cfg)
            .ok_or_else(|| anyhow!("创建 Zipformer OnlineRecognizer 失败"))?;

        println!("[ZIPFORMER] model loaded from {}", dir.display());

        Ok(Self {
            recognizer: Arc::new(recognizer),
        })
    }
}

impl AsrProvider for ZipformerProvider {
    fn name(&self) -> &'static str {
        "zipformer_streaming"
    }

    fn start_streaming(&self, params: AsrStreamParams) -> Result<Box<dyn AsrSession>> {
        let AsrStreamParams {
            audio_rx,
            sample_rate,
            on_update,
        } = params;

        let recognizer = self.recognizer.clone();
        let handle = ZipformerSession::spawn(recognizer, audio_rx, sample_rate, on_update)?;
        Ok(Box::new(ZipformerSession {
            handle: Some(handle),
        }))
    }
}

/// A streaming session: a feeder thread pumps audio, accumulates final text,
/// and emits partials. `stop()` joins the thread and returns the full text.
pub struct ZipformerSession {
    handle: Option<thread::JoinHandle<String>>,
}

/// Append a finalized utterance to the running transcript.
fn push_utterance(full_text: &mut String, text: &str) {
    if !full_text.is_empty() {
        full_text.push(' ');
    }
    full_text.push_str(text);
}

impl ZipformerSession {
    fn spawn(
        recognizer: Arc<OnlineRecognizer>,
        audio_rx: std::sync::mpsc::Receiver<Vec<f32>>,
        sample_rate: u32,
        on_update: Box<dyn Fn(String) + Send + Sync + 'static>,
    ) -> Result<thread::JoinHandle<String>> {
        let needs_resample = (sample_rate as i32 - TARGET_SR).abs() >= 1;
        let mut resampler = if needs_resample {
            Some(StreamingResampler16k::new(sample_rate)?)
        } else {
            None
        };

        let handle = thread::Builder::new()
            .name("zipformer-stream".into())
            .spawn(move || {
                let stream = recognizer.create_stream();
                let mut last_partial = String::new();
                let mut full_text = String::new();

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
                            let mut snapshot = full_text.clone();
                            push_utterance(&mut snapshot, &partial);
                            on_update(snapshot);
                        }
                    }

                    // Endpoint detection: finalize the current utterance.
                    if recognizer.is_endpoint(&stream) {
                        if let Some(result) = recognizer.get_result(&stream) {
                            let text = result.text.trim().to_string();
                            if !text.is_empty() {
                                push_utterance(&mut full_text, &text);
                            }
                        }
                        last_partial.clear();
                        recognizer.reset(&stream);
                    }
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
                        push_utterance(&mut full_text, &text);
                    }
                }

                full_text
            })
            .map_err(|e| anyhow!("failed to spawn zipformer thread: {e}"))?;

        Ok(handle)
    }
}

impl AsrSession for ZipformerSession {
    fn finish_and_wait(mut self: Box<Self>) -> Result<String> {
        // Dropping audio_rx sender (done by caller) causes recv() to return
        // Err, ending the feeder loop. Join to collect the final text.
        if let Some(handle) = self.handle.take() {
            let text = handle
                .join()
                .map_err(|_| anyhow!("zipformer feeder thread panicked"))?;
            Ok(text)
        } else {
            Err(anyhow!("zipformer session already finished"))
        }
    }
}
