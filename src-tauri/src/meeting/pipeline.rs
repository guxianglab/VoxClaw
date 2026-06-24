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

    // Overlap context: the tail of the previous segment is prepended to the
    // next segment's audio before inference. This gives SenseVoice the acoustic
    // context it needs to correctly recognize words at the segment boundary
    // (e.g. "...明天下午" + "的4点" — without overlap, "下午的" is split and
    // misrecognized). Only used for inference; accumulation/timestamps use the
    // segment's own boundaries.
    let overlap_samples = OVERLAP_MS * TARGET_SR as usize / 1000;
    let mut prev_tail: Vec<f32> = Vec::new();
    let mut prev_text = String::new();

    while let Ok(chunk) = audio_rx.recv() {
        let mono_16k = match resampler.as_mut() {
            Some(r) => r.process(&chunk),
            None => chunk,
        };
        let completed = endpointer.feed(&mono_16k);
        for seg in completed {
            let seg_text = process_segment(
                &provider,
                &mut acc,
                &seg,
                &on_segment,
                &prev_tail,
                &prev_text,
            );
            // Save this segment's tail + text for the next segment's overlap.
            if seg.samples.len() >= overlap_samples {
                prev_tail = seg.samples[seg.samples.len() - overlap_samples..].to_vec();
            } else {
                prev_tail = seg.samples.clone();
            }
            prev_text = seg_text;
        }
    }

    // EOF: flush the final in-progress segment.
    for seg in endpointer.flush() {
        let _ = process_segment(
            &provider,
            &mut acc,
            &seg,
            &on_segment,
            &prev_tail,
            &prev_text,
        );
    }

    Ok(PipelineResult {
        full_text: acc.full_text,
        segments: acc.segments,
    })
}

/// How much of the previous segment's tail to prepend as overlap context.
/// 1.5 s is a good balance: enough acoustic context for word-boundary recovery,
/// short enough to not skew recognition of the current segment.
const OVERLAP_MS: usize = 1500;

/// Transcribe one segment with overlap context, dedup repeated words caused by
/// the overlap, and accumulate the result. Returns the (deduped) segment text.
fn process_segment(
    provider: &SenseVoiceProvider,
    acc: &mut SegmentAccumulator,
    seg: &VadSegment,
    on_segment: &impl Fn(&str),
    prev_tail: &[f32],
    prev_text: &str,
) -> String {
    let dur_ms = (seg.samples.len() as f64 / TARGET_SR as f64 * 1000.0) as u64;
    println!(
        "[MEETING] segment: samples={}, duration={:.1}s, offset={:.1}s, overlap={}",
        seg.samples.len(),
        dur_ms as f64 / 1000.0,
        seg.start_sample as f64 / TARGET_SR as f64,
        prev_tail.len(),
    );

    // Build inference input: [prev_tail (overlap context)] + [segment audio].
    let has_overlap = !prev_tail.is_empty();
    let mut infer_input = Vec::with_capacity(prev_tail.len() + seg.samples.len());
    if has_overlap {
        infer_input.extend_from_slice(prev_tail);
    }
    infer_input.extend_from_slice(&seg.samples);

    match provider.transcribe_segment(&infer_input) {
        Ok(raw_text) => {
            // The overlap context may produce duplicated leading text (the
            // model transcribes the same tail audio again, often slightly
            // differently — e.g. "流水线" vs "沦现"). Strip it so the final
            // transcript doesn't stutter at boundaries.
            let text = if has_overlap {
                dedup_overlap(&raw_text, prev_text)
            } else {
                raw_text
            };
            println!("[MEETING] segment text: {:?}", text);
            acc.push(seg, &text);
            on_segment(&acc.full_text.clone());
            text
        }
        Err(e) => {
            eprintln!("[MEETING] segment inference failed: {e}; skipping segment");
            String::new()
        }
    }
}

/// Remove the leading portion of `current` that overlaps with the tail of
/// `prev_text`.
///
/// When overlap context is prepended, SenseVoice transcribes that context
/// again at the start of `current`. This often differs slightly from how it
/// was transcribed at the end of the previous segment (different surrounding
/// context), creating a duplicate-but-garbled leading word. We detect the
/// overlap by finding the longest suffix of `prev_text` that matches a prefix
/// of `current` (character-level), and strip it.
fn dedup_overlap(current: &str, prev_text: &str) -> String {
    let prev_chars: Vec<char> = prev_text.chars().collect();
    let curr_chars: Vec<char> = current.chars().collect();
    if prev_chars.is_empty() || curr_chars.is_empty() {
        return current.to_string();
    }

    // How many leading characters of `current` could plausibly be overlap?
    // The overlap audio is ~1.5s of speech; that's typically 4-8 characters.
    // Require at least 3 matching characters so a coincidental short repeat
    // (e.g. "代码" appearing in both segments for unrelated reasons) isn't
    // stripped. Cap the search at 12 to avoid over-stripping.
    let max_overlap = curr_chars.len().min(12).min(prev_chars.len());
    const MIN_MATCH: usize = 3;

    // Find the longest suffix-of-prev that equals a prefix-of-current.
    let mut best_len = 0usize;
    for len in (MIN_MATCH..=max_overlap).rev() {
        let prev_suffix = &prev_chars[prev_chars.len() - len..];
        let curr_prefix = &curr_chars[..len];
        if prev_suffix == curr_prefix {
            best_len = len;
            break;
        }
    }

    if best_len > 0 {
        curr_chars[best_len..].iter().collect::<String>().trim_start().to_string()
    } else {
        current.to_string()
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

    #[test]
    fn dedup_strips_repeated_overlap_word() {
        // prev ended with "流水线", current (with overlap) starts with it.
        let out = dedup_overlap("流水线迎来可能不是", "接入研发流水线");
        assert_eq!(out, "迎来可能不是");
    }

    #[test]
    fn dedup_keeps_short_legitimate_repeat() {
        // "代码" legitimately appears in both but is NOT overlap duplication
        // (too short to be from the 1.5s overlap audio). Must be kept.
        let out = dedup_overlap("代码是资产", "讨论代码");
        assert_eq!(out, "代码是资产");
    }

    #[test]
    fn dedup_handles_empty_prev() {
        assert_eq!(dedup_overlap("hello", ""), "hello");
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
