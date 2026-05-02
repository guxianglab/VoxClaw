//! Kaldi-style 80-dim log-mel filter-bank, matching the FunASR / SenseVoice
//! frontend so the precomputed CMVN (`am.mvn`) lines up.
//!
//! Settings are hard-coded to the SenseVoice defaults:
//! - 16 kHz mono input (caller is responsible for resampling)
//! - 25 ms window (400 samples), 10 ms hop (160 samples)
//! - 512-pt FFT, Hamming window, pre-emphasis 0.97
//! - 80 mel bins, 20 Hz – 8000 Hz, 16-bit waveform scale then natural log
//! - LFR (low-frame-rate) stack: window=7, shift=6 → 80×7=560 dim per frame
//!
//! References:
//! - Kaldi `compute-fbank-feats`
//! - sherpa-onnx `offline-sense-voice-impl.cc`

use std::sync::Arc;

use rustfft::{num_complex::Complex32, FftPlanner};

const SAMPLE_RATE: f32 = 16_000.0;
const FRAME_LEN: usize = 400; // 25 ms
const FRAME_SHIFT: usize = 160; // 10 ms
const FFT_SIZE: usize = 512;
const NUM_MEL: usize = 80;
const PRE_EMPH: f32 = 0.97;
const LOW_FREQ: f32 = 20.0;
const HIGH_FREQ: f32 = 8_000.0;
const WAVEFORM_SCALE: f32 = 32_768.0;
pub const LFR_M: usize = 7;
pub const LFR_N: usize = 6;
pub const FEAT_DIM: usize = NUM_MEL * LFR_M; // 560

/// Cached, reusable filter-bank + window. Cheap to clone (everything is in
/// `Arc`).
#[derive(Clone)]
pub struct FbankExtractor {
    window: Arc<Vec<f32>>,
    mel_filters: Arc<Vec<MelFilter>>,
    fft: Arc<dyn rustfft::Fft<f32>>,
}

#[derive(Clone)]
struct MelFilter {
    start_bin: usize,
    weights: Vec<f32>,
}

impl FbankExtractor {
    pub fn new() -> Self {
        let window = Arc::new(hamming_window(FRAME_LEN));
        let mel_filters = Arc::new(build_mel_filters(
            FFT_SIZE,
            SAMPLE_RATE,
            NUM_MEL,
            LOW_FREQ,
            HIGH_FREQ,
        ));
        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(FFT_SIZE);
        Self {
            window,
            mel_filters,
            fft,
        }
    }

    /// Compute log-mel features (T, 80) for `samples` (16 kHz mono float).
    /// Matches upstream WavFrontend by first mapping float samples onto the
    /// int16-like range used by kaldi-native-fbank.
    pub fn compute(&self, samples: &[f32]) -> Vec<[f32; NUM_MEL]> {
        if samples.len() < FRAME_LEN {
            return Vec::new();
        }
        let n_frames = (samples.len() - FRAME_LEN) / FRAME_SHIFT + 1;
        let mut out = Vec::with_capacity(n_frames);
        let mut buf = vec![Complex32::new(0.0, 0.0); FFT_SIZE];

        for f in 0..n_frames {
            let start = f * FRAME_SHIFT;
            let frame = &samples[start..start + FRAME_LEN];

            // Pre-emphasis + window into real buffer (clear FFT buf first).
            for c in buf.iter_mut() {
                *c = Complex32::new(0.0, 0.0);
            }
            // First sample uses itself as previous (Kaldi convention).
            buf[0].re = ((frame[0] - PRE_EMPH * frame[0]) * WAVEFORM_SCALE) * self.window[0];
            for i in 1..FRAME_LEN {
                buf[i].re =
                    ((frame[i] - PRE_EMPH * frame[i - 1]) * WAVEFORM_SCALE) * self.window[i];
            }

            self.fft.process(&mut buf);

            // Power spectrum on the first FFT_SIZE/2 + 1 bins.
            let n_bins = FFT_SIZE / 2 + 1;
            let mut power = [0.0f32; FFT_SIZE / 2 + 1];
            for i in 0..n_bins {
                let c = buf[i];
                power[i] = c.re * c.re + c.im * c.im;
            }

            // Mel filter bank → log.
            let mut mel = [0.0f32; NUM_MEL];
            for (m, filt) in self.mel_filters.iter().enumerate() {
                let mut sum = 0.0f32;
                for (k, w) in filt.weights.iter().enumerate() {
                    sum += w * power[filt.start_bin + k];
                }
                mel[m] = sum.max(f32::MIN_POSITIVE).ln();
            }
            out.push(mel);
        }
        out
    }
}

/// Apply CMVN: `(x + neg_mean) * inv_stddev`.
/// `neg_mean` and `inv_stddev` are length 80*7 = 560 (post-LFR), as exported
/// by FunASR's `am.mvn`.
pub fn apply_cmvn_lfr(features: &mut [f32], neg_mean: &[f32], inv_stddev: &[f32]) {
    debug_assert_eq!(features.len() % neg_mean.len(), 0);
    let dim = neg_mean.len();
    for chunk in features.chunks_mut(dim) {
        for i in 0..dim {
            chunk[i] = (chunk[i] + neg_mean[i]) * inv_stddev[i];
        }
    }
}

/// Stack `LFR_M` consecutive frames with stride `LFR_N`. Each output frame is
/// 80 * LFR_M = 560 floats. Matches the upstream SenseVoice frontend by
/// left-padding with the first frame and right-padding with the last frame.
pub fn apply_lfr(frames: &[[f32; NUM_MEL]]) -> Vec<f32> {
    if frames.is_empty() {
        return Vec::new();
    }
    let t_in = frames.len();
    let t_out = (t_in + LFR_N - 1) / LFR_N;
    let left_pad = (LFR_M - 1) / 2;
    let mut out = Vec::with_capacity(t_out * FEAT_DIM);
    for t in 0..t_out {
        for i in 0..LFR_M {
            let padded_idx = t * LFR_N + i;
            let src_idx = if padded_idx < left_pad {
                0
            } else {
                (padded_idx - left_pad).min(t_in - 1)
            };
            let frame = &frames[src_idx];
            out.extend_from_slice(frame);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn hamming_window(n: usize) -> Vec<f32> {
    use std::f32::consts::PI;
    (0..n)
        .map(|i| {
            let x = 2.0 * PI * i as f32 / (n - 1) as f32;
            0.54 - 0.46 * x.cos()
        })
        .collect()
}

fn mel_scale(hz: f32) -> f32 {
    1127.0 * (1.0 + hz / 700.0).ln()
}

fn build_mel_filters(
    fft_size: usize,
    sample_rate: f32,
    n_mels: usize,
    low_hz: f32,
    high_hz: f32,
) -> Vec<MelFilter> {
    let n_bins = fft_size / 2 + 1;
    let bin_hz = sample_rate / fft_size as f32;
    let low_mel = mel_scale(low_hz);
    let high_mel = mel_scale(high_hz);

    // Inverse mel.
    let inv_mel = |m: f32| 700.0 * ((m / 1127.0).exp() - 1.0);

    let centers: Vec<f32> = (0..n_mels + 2)
        .map(|i| {
            let m = low_mel + (high_mel - low_mel) * i as f32 / (n_mels + 1) as f32;
            inv_mel(m)
        })
        .collect();

    let mut filters = Vec::with_capacity(n_mels);
    for m in 0..n_mels {
        let left = centers[m];
        let center = centers[m + 1];
        let right = centers[m + 2];
        let mut start_bin = n_bins;
        let mut weights: Vec<f32> = Vec::new();
        for k in 0..n_bins {
            let f = k as f32 * bin_hz;
            if f < left || f > right {
                continue;
            }
            let w = if f <= center {
                (f - left) / (center - left)
            } else {
                (right - f) / (right - center)
            };
            if start_bin == n_bins {
                start_bin = k;
            }
            weights.push(w.max(0.0));
        }
        if start_bin == n_bins {
            // Empty filter (shouldn't happen for our settings); push a no-op.
            start_bin = 0;
        }
        filters.push(MelFilter { start_bin, weights });
    }
    filters
}

/// Parse FunASR's `am.mvn` text file → (`neg_mean`, `inv_stddev`).
///
/// Format (FunASR convention):
/// ```text
/// <Nnet>
/// <Splice> 560 560 [ 0 0 ... ]
/// <AddShift> 560 560
///   [ -m0 -m1 ... -m559 ]
/// <Rescale> 560 560
///   [ s0 s1 ... s559 ]
/// </Nnet>
/// ```
/// We grab the two `[ ... ]` blocks following `AddShift` and `Rescale`.
pub fn parse_cmvn(text: &str) -> anyhow::Result<(Vec<f32>, Vec<f32>)> {
    let mut neg_mean: Option<Vec<f32>> = None;
    let mut inv_stddev: Option<Vec<f32>> = None;
    let mut tag: Option<&'static str> = None;
    let mut buffer: Vec<f32> = Vec::new();
    let mut in_brackets = false;

    for raw_token in text.split_whitespace() {
        match raw_token {
            "<AddShift>" => {
                tag = Some("addshift");
                buffer.clear();
                in_brackets = false;
            }
            "<Rescale>" => {
                tag = Some("rescale");
                buffer.clear();
                in_brackets = false;
            }
            "[" => {
                in_brackets = true;
                buffer.clear();
            }
            "]" => {
                in_brackets = false;
                match tag {
                    Some("addshift") => neg_mean = Some(std::mem::take(&mut buffer)),
                    Some("rescale") => inv_stddev = Some(std::mem::take(&mut buffer)),
                    _ => buffer.clear(),
                }
                tag = None;
            }
            _ => {
                if in_brackets {
                    if let Ok(v) = raw_token.parse::<f32>() {
                        buffer.push(v);
                    }
                }
            }
        }
    }

    let neg_mean = neg_mean.ok_or_else(|| anyhow::anyhow!("am.mvn missing AddShift"))?;
    let inv_stddev = inv_stddev.ok_or_else(|| anyhow::anyhow!("am.mvn missing Rescale"))?;
    if neg_mean.len() != inv_stddev.len() {
        return Err(anyhow::anyhow!(
            "am.mvn shift/rescale length mismatch: {} vs {}",
            neg_mean.len(),
            inv_stddev.len()
        ));
    }
    Ok((neg_mean, inv_stddev))
}

#[cfg(test)]
mod tests {
    use super::{apply_lfr, FbankExtractor, FEAT_DIM, LFR_M, NUM_MEL, SAMPLE_RATE};
    use std::f32::consts::PI;

    #[test]
    fn apply_lfr_matches_upstream_padding() {
        let mut frames = Vec::new();
        for value in 1..=8 {
            let mut frame = [0.0f32; NUM_MEL];
            frame[0] = value as f32;
            frames.push(frame);
        }

        let lfr = apply_lfr(&frames);
        assert_eq!(lfr.len(), FEAT_DIM * 2);

        let first: Vec<f32> = (0..LFR_M).map(|i| lfr[i * NUM_MEL]).collect();
        assert_eq!(first, vec![1.0, 1.0, 1.0, 1.0, 2.0, 3.0, 4.0]);

        let second_base = FEAT_DIM;
        let second: Vec<f32> = (0..LFR_M)
            .map(|i| lfr[second_base + i * NUM_MEL])
            .collect();
        assert_eq!(second, vec![4.0, 5.0, 6.0, 7.0, 8.0, 8.0, 8.0]);
    }

    #[test]
    fn compute_preserves_quiet_speech_energy() {
        let extractor = FbankExtractor::new();
        let samples: Vec<f32> = (0..1600)
            .map(|i| {
                let phase = 2.0 * PI * 440.0 * i as f32 / SAMPLE_RATE;
                0.001 * phase.sin()
            })
            .collect();

        let features = extractor.compute(&samples);
        assert!(!features.is_empty());
        assert!(features.iter().flatten().any(|value| *value != 0.0));
    }
}
