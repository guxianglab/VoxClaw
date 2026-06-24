//! Silero VAD inference + endpoint detector.
//!
//! Two layers:
//! - [`SileroVad`] wraps the ONNX model and scores one 512-sample chunk.
//! - [`VadEndpointer`] is a pure-logic state machine that converts a stream of
//!   per-chunk probabilities into completed speech segments. Its probability
//!   source is injected, so the endpointer is fully unit-testable without a
//!   model.
//!
//! Both together replace the "buffer the whole meeting then infer once"
//! strategy that hung on long recordings.

use std::path::Path;
use std::sync::Mutex;

use anyhow::{anyhow, Result};
use ndarray::Array2;
use ort::{
    ep::directml::DirectML,
    session::{builder::GraphOptimizationLevel, Session},
    value::Tensor,
};

const VAD_CHUNK: usize = 512; // 32 ms @ 16 kHz — Silero fixed frame size
const VAD_CONTEXT: usize = 64; // prepended context for the stateless export
const VAD_INPUT_LEN: usize = VAD_CHUNK + VAD_CONTEXT; // 576

/// A completed speech segment, in the 16 kHz sample domain.
#[derive(Clone, Debug, PartialEq)]
pub struct VadSegment {
    pub samples: Vec<f32>,
    pub start_sample: u64,
    pub end_sample: u64,
}

/// Tunable endpoint-detection parameters. Defaults follow Silero/sherpa-onnx
/// recommended values for general speech.
#[derive(Clone, Debug)]
pub struct VadEndpointerConfig {
    pub threshold: f32,
    pub min_silence_samples: u64,
    pub speech_pad_samples: u64,
    pub max_segment_samples: u64,
}

impl Default for VadEndpointerConfig {
    fn default() -> Self {
        // 16 kHz constants.
        // min_silence 500 ms = 8000 samples; speech_pad 200 ms = 3200 samples;
        // max_segment 30 s = 480000 samples.
        Self {
            threshold: 0.5,
            min_silence_samples: 8_000,
            speech_pad_samples: 3_200,
            max_segment_samples: 480_000,
        }
    }
}

/// Converts a stream of per-chunk speech probabilities into completed speech
/// segments. Pure logic — the ONNX VAD model is injected as a probability
/// source so this is fully unit-testable without a model.
pub struct VadEndpointer<F: FnMut(&[f32]) -> f32> {
    cfg: VadEndpointerConfig,
    prob_source: F,
    // Running state.
    in_speech: bool,
    speech_start_sample: u64,  // absolute offset where current speech began
    current_segment: Vec<f32>, // accumulating samples of current speech
    silence_run: u64,          // consecutive silent samples within speech
    total_seen: u64,           // absolute sample count consumed
    chunk_buffer: Vec<f32>,    // sub-512 accumulations before a VAD call
    pre_roll: Vec<f32>,        // ring of samples kept to pad speech onset
}

impl<F: FnMut(&[f32]) -> f32> VadEndpointer<F> {
    const CHUNK: usize = VAD_CHUNK;

    pub fn new(cfg: VadEndpointerConfig, prob_source: F) -> Self {
        Self {
            cfg,
            prob_source,
            in_speech: false,
            speech_start_sample: 0,
            current_segment: Vec::new(),
            silence_run: 0,
            total_seen: 0,
            chunk_buffer: Vec::new(),
            pre_roll: Vec::new(),
        }
    }

    /// Feed arbitrary-length 16 kHz samples. Returns zero or more completed
    /// segments. `prob_source` is called once per 512-sample chunk.
    pub fn feed(&mut self, samples: &[f32]) -> Vec<VadSegment> {
        let mut out = Vec::new();
        self.chunk_buffer.extend_from_slice(samples);
        while self.chunk_buffer.len() >= Self::CHUNK {
            let chunk: Vec<f32> = self.chunk_buffer.drain(..Self::CHUNK).collect();
            let prob = (self.prob_source)(&chunk);
            out.extend(self.process_chunk(&chunk, prob));
        }
        out
    }

    /// Flush: emit any in-progress segment. Called once at stream end.
    pub fn flush(&mut self) -> Vec<VadSegment> {
        let mut out = Vec::new();
        if self.in_speech && !self.current_segment.is_empty() {
            out.push(self.finalize_segment());
        }
        // Drain any residual sub-512 samples (already buffered in
        // current_segment if in speech; otherwise discarded).
        self.chunk_buffer.clear();
        out
    }

    fn process_chunk(&mut self, chunk: &[f32], prob: f32) -> Vec<VadSegment> {
        let chunk_len = chunk.len() as u64;
        let mut out = Vec::new();
        let is_speech = prob >= self.cfg.threshold;

        // Maintain a pre-roll ring so we can pad speech onset with context.
        if !self.in_speech {
            self.pre_roll.extend_from_slice(chunk);
            let excess = self.pre_roll.len() as i64 - self.cfg.speech_pad_samples as i64;
            if excess > 0 {
                self.pre_roll.drain(..excess as usize);
            }
        }

        if !self.in_speech && is_speech {
            // Speech onset.
            self.in_speech = true;
            self.speech_start_sample =
                self.total_seen.saturating_sub(self.cfg.speech_pad_samples);
            self.current_segment.clear();
            self.current_segment.extend_from_slice(&self.pre_roll);
            self.current_segment.extend_from_slice(chunk);
            self.silence_run = 0;
        } else if self.in_speech {
            self.current_segment.extend_from_slice(chunk);
            if is_speech {
                self.silence_run = 0;
            } else {
                self.silence_run += chunk_len;
            }
        }

        self.total_seen += chunk_len;

        if self.in_speech {
            // Force-split if segment grew too long.
            if self.current_segment.len() as u64 >= self.cfg.max_segment_samples {
                out.push(self.finalize_segment());
            } else if self.silence_run >= self.cfg.min_silence_samples {
                out.push(self.finalize_segment());
            }
        }

        out
    }

    fn finalize_segment(&mut self) -> VadSegment {
        let end_sample = self.total_seen;
        let start_sample = self.speech_start_sample;
        let samples = std::mem::take(&mut self.current_segment);
        self.in_speech = false;
        self.silence_run = 0;
        self.pre_roll.clear();
        VadSegment {
            samples,
            start_sample,
            end_sample,
        }
    }
}

/// Hand-written Silero VAD inference over ONNX Runtime. Uses the
/// context-prefix variant of the model (single `input` tensor of shape
/// `[1, 576]`); cross-chunk continuity is approximated by prepending the last
/// 64 samples of the previous chunk.
///
/// The global ONNX Runtime environment is initialised once by
/// `SenseVoiceProvider::try_new` before any meeting starts, so this wrapper
/// only creates a `Session` (which is safe to do multiple times).
pub struct SileroVad {
    session: Mutex<Session>,
    context: Mutex<Vec<f32>>,
}

impl SileroVad {
    /// `use_gpu` mirrors the SenseVoice setting so VAD can use DirectML too.
    pub fn try_new(model_path: &Path, use_gpu: bool) -> Result<Self> {
        let mut builder = Session::builder().map_err(|e| anyhow!("vad ort builder: {e}"))?;
        builder = builder
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|e| anyhow!("vad ort opt: {e}"))?;
        builder = builder
            .with_intra_threads(vad_num_threads())
            .map_err(|e| anyhow!("vad ort threads: {e}"))?;

        let session = if use_gpu {
            match builder
                .clone()
                .with_execution_providers([DirectML::default().build()])
                .and_then(|b| b.commit_from_file(model_path))
            {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("[VAD] DirectML failed ({}), CPU fallback", e);
                    builder
                        .commit_from_file(model_path)
                        .map_err(|e| anyhow!("vad load model (CPU): {e}"))?
                }
            }
        } else {
            builder
                .commit_from_file(model_path)
                .map_err(|e| anyhow!("vad load model: {e}"))?
        };

        println!("[VAD] model loaded from {}", model_path.display());

        Ok(Self {
            session: Mutex::new(session),
            context: Mutex::new(vec![0.0; VAD_CONTEXT]),
        })
    }

    /// Score one 512-sample chunk. Returns speech probability in `[0,1]`.
    pub fn process_chunk(&self, chunk: &[f32]) -> Result<f32> {
        if chunk.len() != VAD_CHUNK {
            return Err(anyhow!(
                "vad chunk must be {} samples, got {}",
                VAD_CHUNK,
                chunk.len()
            ));
        }

        // Assemble [context(64) | chunk(512)] = 576.
        let mut input = Vec::with_capacity(VAD_INPUT_LEN);
        {
            let ctx = self.context.lock().map_err(|_| anyhow!("vad ctx poisoned"))?;
            input.extend_from_slice(&ctx);
        }
        input.extend_from_slice(chunk);

        // Update context for the next call: last VAD_CONTEXT samples of this chunk.
        {
            let mut ctx = self.context.lock().map_err(|_| anyhow!("vad ctx poisoned"))?;
            let off = chunk.len() - VAD_CONTEXT;
            ctx.copy_from_slice(&chunk[off..]);
        }

        let tensor_input = Array2::from_shape_vec((1, VAD_INPUT_LEN), input)
            .map_err(|e| anyhow!("vad reshape: {e}"))?;
        let tensor = Tensor::from_array(tensor_input).map_err(|e| anyhow!("vad input tensor: {e}"))?;

        let mut session = self
            .session
            .lock()
            .map_err(|_| anyhow!("vad session poisoned"))?;
        let outputs = session
            .run(ort::inputs!["input" => tensor])
            .map_err(|e| anyhow!("vad run: {e}"))?;

        let (_name, value) = outputs
            .iter()
            .next()
            .ok_or_else(|| anyhow!("vad: no outputs"))?;
        let logits = value
            .try_extract_array::<f32>()
            .map_err(|e| anyhow!("vad extract: {e}"))?;
        let prob = logits
            .as_slice()
            .ok_or_else(|| anyhow!("vad output not contiguous"))?
            .first()
            .copied()
            .unwrap_or(0.0);
        Ok(prob.clamp(0.0, 1.0))
    }

    /// Reset the cross-chunk context (e.g. for a new session). Currently each
    /// meeting builds a fresh `SileroVad`, so this is reserved for callers that
    /// reuse one instance across sessions.
    #[allow(dead_code)]
    pub fn reset(&self) {
        if let Ok(mut ctx) = self.context.lock() {
            ctx.fill(0.0);
        }
    }
}

fn vad_num_threads() -> usize {
    std::thread::available_parallelism()
        .map(|n| (n.get() / 2).max(1))
        .unwrap_or(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build an endpointer whose prob source returns probs from a Vec, in
    /// order, defaulting to 0.0 when exhausted (test-only).
    fn endpoint_with_probs(probs: Vec<f32>) -> VadEndpointer<impl FnMut(&[f32]) -> f32> {
        let mut idx = 0usize;
        VadEndpointer::new(
            VadEndpointerConfig {
                // Tiny values so tests don't need huge buffers.
                min_silence_samples: 512,  // 1 silent chunk triggers split
                speech_pad_samples: 0,     // disable padding for predictability
                max_segment_samples: 2_048, // 4 chunks
                threshold: 0.5,
            },
            move |_| {
                let p = probs.get(idx).copied().unwrap_or(0.0);
                idx += 1;
                p
            },
        )
    }

    #[test]
    fn pure_silence_produces_no_segments() {
        let mut ep = endpoint_with_probs(vec![0.1, 0.1, 0.1, 0.1]);
        let segs = ep.feed(&[0.0; 2048]);
        assert!(segs.is_empty(), "no speech should yield no segments");
        assert!(ep.flush().is_empty());
    }

    #[test]
    fn speech_then_silence_yields_one_segment() {
        // probs: speech, speech, silence, silence (4 chunks = 2048 samples)
        let mut ep = endpoint_with_probs(vec![0.9, 0.9, 0.1, 0.1]);
        let mut all = ep.feed(&[0.0; 2048]);
        all.extend(ep.flush());
        assert_eq!(all.len(), 1);
        // start_sample should be 0 (no pad), end at 2048.
        assert_eq!(all[0].start_sample, 0);
        assert!(!all[0].samples.is_empty());
    }

    #[test]
    fn continuous_speech_forces_split_at_max_length() {
        // 6 chunks all speech, max_segment = 2048 (4 chunks) → forced split(s).
        let mut ep = endpoint_with_probs(vec![0.9; 6]);
        let mut all = ep.feed(&[0.0; 6 * 512]);
        all.extend(ep.flush());
        assert!(
            all.len() >= 2,
            "should force-split long speech, got {} segments",
            all.len()
        );
    }

    #[test]
    fn flush_emits_in_progress_segment() {
        // speech, speech, then flush (no trailing silence).
        let mut ep = endpoint_with_probs(vec![0.9, 0.9]);
        let segs = ep.feed(&[0.0; 1024]);
        assert!(segs.is_empty(), "no split expected mid-speech");
        let flushed = ep.flush();
        assert_eq!(flushed.len(), 1);
        assert!(!flushed[0].samples.is_empty());
    }
}
