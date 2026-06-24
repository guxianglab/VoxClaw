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
use ndarray::{Array2, Array3};
use ort::{
    ep::directml::DirectML,
    session::{builder::GraphOptimizationLevel, Session},
    value::Tensor,
};

const VAD_CHUNK: usize = 512; // 32 ms @ 16 kHz — Silero fixed frame size

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
        // 16 kHz constants, tuned for continuous speech (meetings/lectures).
        //
        // Over-segmentation (splitting one sentence into many fragments) hurts
        // ASR accuracy badly: each fragment loses the surrounding context, so
        // word boundaries get misrecognized (e.g. "举头" → "去的" + "头望").
        // To avoid this we require a long, confident silence before splitting:
        //
        //   min_silence 2000 ms = 32000 samples — only real pauses (sentence/
        //     paragraph breaks, not breaths or short phrase-end pauses) split.
        //     Natural breaths/换气 are typically 200-800 ms, well below this.
        //   speech_pad 300 ms = 4800 samples — keep a little silence at each
        //     segment edge so the trailing word isn't clipped.
        //   threshold 0.5 — standard Silero default; balanced sensitivity.
        //   max_segment 30 s = 480000 samples — hard cap to stay in the
        //     SenseVoice comfort zone even with no silence.
        Self {
            threshold: 0.5,
            min_silence_samples: 32_000,
            speech_pad_samples: 4_800,
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

/// Hand-written Silero VAD v4 inference over ONNX Runtime.
///
/// This matches the sherpa-onnx `silero_vad.onnx` export (stateful v4): the
/// model takes an audio chunk `x [1,512]` plus the LSTM state `h [2,1,64]` and
/// `c [2,1,64]`, and returns the speech probability plus the updated state
/// `new_h` / `new_c`. The state is threaded chunk-to-chunk so the model has
/// continuity across the whole stream.
///
/// The global ONNX Runtime environment is initialised once by
/// `SenseVoiceProvider::try_new` before any meeting starts, so this wrapper
/// only creates a `Session` (which is safe to do multiple times).
pub struct SileroVad {
    session: Mutex<Session>,
    /// LSTM hidden state h [2,1,64] and cell state c [2,1,64], updated each
    /// call. Guarded by a mutex so the endpointer closure can borrow `&self`.
    state: Mutex<VadState>,
}

struct VadState {
    h: Array3<f32>, // [2, 1, 64] — matches the model's expected rank-3 input
    c: Array3<f32>,
}

impl VadState {
    fn new() -> Self {
        Self {
            h: Array3::zeros((2, 1, 64)),
            c: Array3::zeros((2, 1, 64)),
        }
    }
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
            state: Mutex::new(VadState::new()),
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

        // Snapshot current LSTM state, then drop the lock so we can run inference.
        let (h_arr, c_arr) = {
            let st = self.state.lock().map_err(|_| anyhow!("vad state poisoned"))?;
            (st.h.clone(), st.c.clone())
        };

        let x_input = Array2::from_shape_vec((1, VAD_CHUNK), chunk.to_vec())
            .map_err(|e| anyhow!("vad x reshape: {e}"))?;
        let x_tensor = Tensor::from_array(x_input).map_err(|e| anyhow!("vad x tensor: {e}"))?;
        let h_tensor = Tensor::from_array(h_arr).map_err(|e| anyhow!("vad h tensor: {e}"))?;
        let c_tensor = Tensor::from_array(c_arr).map_err(|e| anyhow!("vad c tensor: {e}"))?;

        let mut session = self
            .session
            .lock()
            .map_err(|_| anyhow!("vad session poisoned"))?;
        let outputs = session
            .run(ort::inputs!["x" => x_tensor, "h" => h_tensor, "c" => c_tensor])
            .map_err(|e| anyhow!("vad run: {e}"))?;

        // Collect outputs by name. The model returns prob, new_h, new_c.
        let mut prob: f32 = 0.0;
        let mut new_h: Option<Array3<f32>> = None;
        let mut new_c: Option<Array3<f32>> = None;
        for (name, value) in outputs.iter() {
            let arr = match value.try_extract_array::<f32>() {
                Ok(a) => a,
                Err(_) => continue,
            };
            match name {
                "prob" => {
                    prob = arr
                        .as_slice()
                        .and_then(|s| s.first().copied())
                        .unwrap_or(0.0);
                }
                "new_h" => {
                    // Output shape is [2, 1, 64]; keep it 3D for the next call.
                    new_h = Some(
                        arr.to_owned()
                            .into_shape_with_order((2, 1, 64))
                            .unwrap_or_else(|_| Array3::zeros((2, 1, 64))),
                    );
                }
                "new_c" => {
                    new_c = Some(
                        arr.to_owned()
                            .into_shape_with_order((2, 1, 64))
                            .unwrap_or_else(|_| Array3::zeros((2, 1, 64))),
                    );
                }
                _ => {}
            }
        }

        // Persist the updated state for the next call.
        if let (Some(h), Some(c)) = (new_h, new_c) {
            if let Ok(mut st) = self.state.lock() {
                st.h = h;
                st.c = c;
            }
        }

        Ok(prob.clamp(0.0, 1.0))
    }

    /// Reset the LSTM state to zero (e.g. for a new session). Each meeting
    /// builds a fresh `SileroVad`, so this is reserved for callers that reuse
    /// one instance across sessions.
    #[allow(dead_code)]
    pub fn reset(&self) {
        if let Ok(mut st) = self.state.lock() {
            st.h.fill(0.0);
            st.c.fill(0.0);
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
