//! Streaming 16 kHz resampler with partial-frame carry-over.
//!
//! `rubato::FastFixedIn` requires fixed-size input frames (1024 samples). The
//! audio callback delivers arbitrary chunk sizes (Windows WASAPI often hands
//! out 480 samples @ 48 kHz), so a naive caller that zero-pads partial frames
//! and emits the whole padded output time-stretches the stream and inserts a
//! silence gap after every real-audio burst. Streaming ASR (Zipformer /
//! Paraformer) decodes such a choppy feature sequence by re-emitting tokens —
//! the repeated syllables users see ("这这个", "星星星星").
//!
//! This wrapper buffers partial input frames between calls, emits output only
//! for complete frames, and `flush()` trims the final partial frame to its
//! real length.

use anyhow::{anyhow, Result};
use rubato::{FastFixedIn, PolynomialDegree, Resampler};

/// Target sample rate of all sherpa-onnx streaming engines.
pub const TARGET_SR: i32 = 16_000;

/// Input frame size of the underlying `FastFixedIn` resampler.
const FRAME: usize = 1024;

pub struct StreamingResampler16k {
    inner: FastFixedIn<f32>,
    ratio: f64,
    carry: Vec<f32>,
}

impl StreamingResampler16k {
    pub fn new(from_sr: u32) -> Result<Self> {
        let ratio = TARGET_SR as f64 / from_sr as f64;
        let inner = FastFixedIn::<f32>::new(ratio, 1.0, PolynomialDegree::Septic, FRAME, 1)
            .map_err(|e| anyhow!("resampler init: {e}"))?;
        Ok(Self {
            inner,
            ratio,
            carry: Vec::with_capacity(FRAME),
        })
    }

    /// Resample one input chunk. Returns the continuous 16 kHz output for all
    /// complete frames in `samples`; the partial trailing frame is held over
    /// to the next call.
    pub fn process(&mut self, samples: &[f32]) -> Vec<f32> {
        let mut out = Vec::new();
        self.carry.extend_from_slice(samples);
        let mut input_buf: [Vec<f32>; 1] = [Vec::with_capacity(FRAME)];
        let mut idx = 0;
        while idx + FRAME <= self.carry.len() {
            input_buf[0].clear();
            input_buf[0].extend_from_slice(&self.carry[idx..idx + FRAME]);
            if let Ok(processed) = self.inner.process(&input_buf, None) {
                out.extend_from_slice(&processed[0]);
            }
            idx += FRAME;
        }
        if idx > 0 {
            self.carry.drain(..idx);
        }
        out
    }

    /// Flush the remaining partial frame at end-of-stream, trimmed to the real
    /// audio length so no zero-derived samples leak into the output.
    pub fn flush(&mut self) -> Vec<f32> {
        if self.carry.is_empty() {
            return Vec::new();
        }
        let real_len = self.carry.len();
        self.carry.resize(FRAME, 0.0);
        let mut input_buf: [Vec<f32>; 1] = [Vec::with_capacity(FRAME)];
        input_buf[0].extend_from_slice(&self.carry);
        self.carry.clear();
        let mut out = Vec::new();
        if let Ok(processed) = self.inner.process(&input_buf, None) {
            let kept = ((real_len as f64) * self.ratio) as usize;
            out.extend_from_slice(&processed[0][..kept.min(processed[0].len())]);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The old zero-padding bug time-stretched the stream: for 10 chunks of
    /// 480 @ 48 kHz it emitted 10 × 341 ≈ 3410 samples instead of ~1600.
    /// Assert the fixed resampler emits ~1/3 of the input and keeps the
    /// (buffered) real audio length on flush.
    #[test]
    fn stream_resample_does_not_stretch_small_chunks() {
        let mut r = StreamingResampler16k::new(48_000).unwrap();
        let chunk = vec![0.1f32; 480];
        let mut out_len = 0usize;
        for _ in 0..10 {
            out_len += r.process(&chunk).len();
        }
        // 4 full 1024 frames resampled so far; remainder is carried over.
        // (rubato's FIR delay can drop 1 sample from the first frame.)
        assert!((3 * 341..=4 * 341).contains(&out_len), "out_len={out_len}");
        let tail = r.flush();
        assert!(tail.len() >= 200 && tail.len() <= 240, "tail={}", tail.len());
        let total = out_len + tail.len();
        let expected = (4800.0 / 3.0) as usize;
        assert!((total as i64 - expected as i64).abs() <= 8, "total={total} expected≈{expected}");
    }

    #[test]
    fn flush_with_empty_carry_is_noop() {
        let mut r = StreamingResampler16k::new(48_000).unwrap();
        assert!(r.flush().is_empty());
    }
}
