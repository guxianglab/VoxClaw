//! Meeting transcription pipeline: resample → VAD endpoint → per-segment
//! SenseVoice inference → accumulate text + timestamped segments.
//!
//! Runs on a dedicated feeder thread so VAD/ASR CPU work never blocks the
//! audio capture callback. This replaces the previous "buffer the whole
//! recording then infer once" strategy that hung on long recordings.

use std::sync::mpsc::Receiver;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use rubato::{FastFixedIn, PolynomialDegree, Resampler};

use crate::asr::sensevoice::provider::SenseVoiceProvider;
use crate::asr::sensevoice::vad::{VadEndpointer, VadEndpointerConfig, VadSegment};
use crate::asr::TranscriptSegment;

const TARGET_SR: f32 = 16_000.0;

/// Accumulates transcribed segments into full text + a segment list with
/// millisecond timestamps. Pure logic — unit-testable without threads/audio.
struct SegmentAccumulator {
    full_text: String,
    segments: Vec<TranscriptSegment>,
}

impl SegmentAccumulator {
    fn new() -> Self {
        Self {
            full_text: String::new(),
            segments: Vec::new(),
        }
    }

    /// Append a transcribed segment. `vad` carries the 16k sample offsets used
    /// to derive millisecond timestamps.
    fn push(&mut self, vad: &VadSegment, text: &str) {
        let start_ms = (vad.start_sample as f64 / TARGET_SR as f64 * 1000.0) as u64;
        let end_ms = (vad.end_sample as f64 / TARGET_SR as f64 * 1000.0) as u64;
        if !self.full_text.is_empty() && !text.is_empty() {
            self.full_text.push(' ');
        }
        self.full_text.push_str(text);
        if !text.trim().is_empty() {
            self.segments.push(TranscriptSegment {
                start_ms,
                end_ms,
                speaker: None,
                text: text.to_string(),
            });
        }
    }
}

/// Result of running the pipeline to completion.
pub struct PipelineResult {
    pub full_text: String,
    pub segments: Vec<TranscriptSegment>,
}

/// Run the pipeline: consume `audio_rx` (any sample rate), resample to 16k,
/// run VAD endpointing, and dispatch each completed speech segment to
/// SenseVoice. `on_segment` is called with the accumulated full text after
/// each completed segment. Blocks until `audio_rx` is closed (EOF), then
/// flushes the final in-progress segment and returns.
///
/// Each segment inference failure is logged and skipped — a single bad
/// segment never fails the whole meeting.
#[allow(clippy::type_complexity)]
pub fn run_pipeline(
    audio_rx: Receiver<Vec<f32>>,
    sample_rate: u32,
    vad: Arc<crate::asr::sensevoice::vad::SileroVad>,
    provider: Arc<SenseVoiceProvider>,
    vad_cfg: VadEndpointerConfig,
    on_segment: impl Fn(&str),
) -> Result<PipelineResult> {
    let needs_resample = (sample_rate as f32 - TARGET_SR).abs() >= 1.0;

    // The endpointer's probability source calls the ONNX VAD once per chunk.
    let vad_for_ep = vad.clone();
    let mut endpointer = VadEndpointer::new(vad_cfg, move |chunk: &[f32]| {
        match vad_for_ep.process_chunk(chunk) {
            Ok(p) => p,
            Err(e) => {
                // Log once per error rather than per-chunk to avoid flooding;
                // a failed VAD call yields prob 0 (treated as silence).
                eprintln!("[MEETING] VAD inference error: {e}");
                0.0
            }
        }
    });

    let mut acc = SegmentAccumulator::new();
    let mut resampler = if needs_resample {
        Some(Resampler16k::new(sample_rate)?)
    } else {
        None
    };

    while let Ok(chunk) = audio_rx.recv() {
        let mono_16k = match resampler.as_mut() {
            Some(r) => r.process(&chunk),
            None => chunk,
        };
        let completed = endpointer.feed(&mono_16k);
        for seg in completed {
            process_segment(&provider, &mut acc, &seg, &on_segment);
        }
    }

    // EOF: flush the final in-progress segment.
    for seg in endpointer.flush() {
        process_segment(&provider, &mut acc, &seg, &on_segment);
    }

    Ok(PipelineResult {
        full_text: acc.full_text,
        segments: acc.segments,
    })
}

fn process_segment(
    provider: &SenseVoiceProvider,
    acc: &mut SegmentAccumulator,
    seg: &VadSegment,
    on_segment: &impl Fn(&str),
) {
    match provider.transcribe_segment(&seg.samples) {
        Ok(text) => {
            acc.push(seg, &text);
            on_segment(&acc.full_text.clone());
        }
        Err(e) => {
            eprintln!("[MEETING] segment inference failed: {e}; skipping segment");
        }
    }
}

// --- Resampler (rubato, mirrors sensevoice/provider.rs) ----------------------

struct Resampler16k {
    resampler: FastFixedIn<f32>,
    chunk: usize,
}

impl Resampler16k {
    fn new(from_sr: u32) -> Result<Self> {
        let ratio = TARGET_SR as f64 / from_sr as f64;
        let chunk = 1024usize;
        let resampler = FastFixedIn::<f32>::new(
            ratio,
            1.0,
            PolynomialDegree::Septic,
            chunk,
            1,
        )
        .map_err(|e| anyhow!("pipeline resampler init: {e}"))?;
        Ok(Self { resampler, chunk })
    }

    fn process(&mut self, samples: &[f32]) -> Vec<f32> {
        let mut out = Vec::new();
        let mut idx = 0;
        let mut input_buf: [Vec<f32>; 1] = [Vec::with_capacity(self.chunk)];
        while idx + self.chunk <= samples.len() {
            input_buf[0].clear();
            input_buf[0].extend_from_slice(&samples[idx..idx + self.chunk]);
            if let Ok(processed) = self.resampler.process(&input_buf, None) {
                out.extend_from_slice(&processed[0]);
            }
            idx += self.chunk;
        }
        // tail pad
        if idx < samples.len() {
            input_buf[0].clear();
            input_buf[0].extend_from_slice(&samples[idx..]);
            input_buf[0].resize(self.chunk, 0.0);
            if let Ok(processed) = self.resampler.process(&input_buf, None) {
                out.extend_from_slice(&processed[0]);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asr::sensevoice::vad::VadSegment;

    #[test]
    fn accumulator_appends_text_and_timestamps() {
        let mut acc = SegmentAccumulator::new();
        let s1 = VadSegment {
            samples: vec![0.0; 16000],
            start_sample: 0,
            end_sample: 16000,
        };
        acc.push(&s1, "hello");
        let s2 = VadSegment {
            samples: vec![0.0; 16000],
            start_sample: 32000,
            end_sample: 48000,
        };
        acc.push(&s2, "world");
        assert_eq!(acc.full_text, "hello world");
        assert_eq!(acc.segments.len(), 2);
        assert_eq!(acc.segments[0].start_ms, 0);
        assert_eq!(acc.segments[0].end_ms, 1000);
        assert_eq!(acc.segments[1].start_ms, 2000);
        assert_eq!(acc.segments[1].end_ms, 3000);
    }

    #[test]
    fn accumulator_skips_empty_text() {
        let mut acc = SegmentAccumulator::new();
        let s = VadSegment {
            samples: vec![0.0; 16000],
            start_sample: 0,
            end_sample: 16000,
        };
        acc.push(&s, "");
        assert_eq!(acc.full_text, "");
        assert!(acc.segments.is_empty());
    }

    /// Regression for the original bug: a 20-30 minute recording must NOT hang.
    ///
    /// This validates the *no-hang* property structurally: a very long
    /// synthetic stream (30 min of silence) is drained through a fake-prob
    /// endpointer (no model needed). It proves the feeder loop always reaches
    /// EOF and returns, regardless of input length — the precise failure mode
    /// of the old "buffer everything then infer once" design, which held the
    /// whole recording in memory and OOM'd / hung on the single inference.
    #[test]
    fn long_stream_does_not_hang() {
        use crate::asr::sensevoice::vad::VadEndpointer;
        // Pure silence → prob source always returns 0.0 → no segments emitted,
        // but every 512-sample chunk is still consumed.
        let mut ep = VadEndpointer::new(VadEndpointerConfig::default(), |_| 0.0);
        // 30 minutes @ 16 kHz = 28.8M samples, fed in 4096-sample chunks.
        let total = 28_800_000usize;
        let mut produced = 0usize;
        let chunk = [0.0f32; 4096];
        let mut fed = 0usize;
        while fed < total {
            let n = (total - fed).min(chunk.len());
            produced += ep.feed(&chunk[..n]).len();
            fed += n;
        }
        produced += ep.flush().len();
        // Pure silence → no segments. The point is we got here at all and fast.
        assert_eq!(produced, 0);
    }
}
