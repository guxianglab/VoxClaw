//! Speech-enhancement pre-filter for the ASR pipeline.
//!
//! Sits *between* the audio capture (mic capture / meeting mixer) and the ASR
//! provider. When enabled, each captured f32 chunk is passed through a
//! denoiser (sherpa-onnx `OnlineSpeechDenoiser`, GTCRN or DPDFNet) before it
//! reaches the recognizer, suppressing stationary noise so dictation accuracy
//! improves in noisy environments.
//!
//! ## Pluggability
//! [`AudioEnhancer`] is a trait so the pipeline stays enhancement-agnostic:
//! - [`NoOpEnhancer`] passes samples through unchanged (the default, current
//!   behaviour — zero-cost).
//! - [`SherpaEnhancer`] wraps `sherpa_onnx::OnlineSpeechDenoiser`. The model
//!   is loaded lazily on first use, and resampling is handled automatically
//!   because the denoiser expects its own native sample rate (GTCRN = 16 kHz).
//!
//! This module is intentionally wired as an opt-in experiment (see
//! `AppConfig.audio_enhancement`). It does not alter the default path until a
//! user explicitly enables it and provides a denoiser model path.

use anyhow::{anyhow, Result};
use rubato::{FastFixedIn, PolynomialDegree, Resampler};
use sherpa_onnx::{
    OfflineSpeechDenoiserModelConfig, OnlineSpeechDenoiser, OnlineSpeechDenoiserConfig,
};

use crate::storage::AudioEnhancementConfig;

/// A chunk-level audio transform applied before ASR. Implementations must be
/// `Send` because the meeting/ dictation feeder threads hold them across
/// `mpsc::recv` calls.
pub trait AudioEnhancer: Send {
    /// Transform one chunk of mono f32 samples at `sample_rate`. Returns the
    /// (possibly resampled-back) enhanced chunk. Implementations may buffer a
    /// small amount of state between calls.
    fn enhance(&mut self, samples: &[f32], sample_rate: u32) -> Vec<f32>;

    /// Human-readable name for logging.
    fn name(&self) -> &'static str;
}

/// Pass-through enhancer — the zero-cost default.
pub struct NoOpEnhancer;

impl AudioEnhancer for NoOpEnhancer {
    fn enhance(&mut self, samples: &[f32], _sample_rate: u32) -> Vec<f32> {
        samples.to_vec()
    }

    fn name(&self) -> &'static str {
        "none"
    }
}

/// sherpa-onnx streaming speech denoiser (GTCRN by default). Loads the model
/// lazily, resamples to the denoiser's native rate, denoises, and resamples
/// back to the caller's rate so downstream code is unchanged.
pub struct SherpaEnhancer {
    denoiser: OnlineSpeechDenoiser,
    /// Native sample rate the denoiser expects (queried from the model).
    native_sr: i32,
    /// Resampler caller_sr → native_sr, built lazily on first call.
    in_resampler: Option<FastFixedIn<f32>>,
    /// Resampler native_sr → caller_sr.
    out_resampler: Option<FastFixedIn<f32>>,
    last_caller_sr: u32,
}

impl SherpaEnhancer {
    /// Build a denoiser from config. The GTCRN model path is required; DPDFNet
    /// can be configured via the same config struct if present.
    pub fn try_new(config: &AudioEnhancementConfig) -> Result<Self> {
        let mut model = OfflineSpeechDenoiserModelConfig::default();
        if !config.gtcrn_model.is_empty() {
            model.gtcrn.model = Some(config.gtcrn_model.clone());
        }
        if !config.dpdfnet_model.is_empty() {
            model.dpdfnet.model = Some(config.dpdfnet_model.clone());
        }
        if model.gtcrn.model.is_none() && model.dpdfnet.model.is_none() {
            return Err(anyhow!(
                "未配置语音增强模型。请在设置中填写 GTCRN 或 DPDFNet 模型路径。"
            ));
        }
        model.num_threads = std::thread::available_parallelism()
            .map(|n| (n.get() / 2).max(1) as i32)
            .unwrap_or(2);

        let variant_name = if model.gtcrn.model.is_some() {
            "GTCRN"
        } else {
            "DPDFNet"
        };
        let cfg = OnlineSpeechDenoiserConfig { model };
        let denoiser = OnlineSpeechDenoiser::create(&cfg)
            .ok_or_else(|| anyhow!("创建 sherpa-onnx OnlineSpeechDenoiser 失败"))?;
        let native_sr = denoiser.sample_rate();
        println!("[ENHANCE] {} denoiser loaded (native {} Hz)", variant_name, native_sr);
        Ok(Self {
            denoiser,
            native_sr,
            in_resampler: None,
            out_resampler: None,
            last_caller_sr: 0,
        })
    }
}

impl AudioEnhancer for SherpaEnhancer {
    fn enhance(&mut self, samples: &[f32], sample_rate: u32) -> Vec<f32> {
        // (Re)build resamplers if the caller's rate changed since last call.
        if sample_rate != self.last_caller_sr {
            let (r#in, out) = match build_resampler_pair(self.native_sr, sample_rate as i32) {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("[ENHANCE] resampler rebuild failed: {e}; passing through");
                    return samples.to_vec();
                }
            };
            self.in_resampler = Some(r#in);
            self.out_resampler = Some(out);
            self.last_caller_sr = sample_rate;
        }

        // caller_sr → native_sr
        let native_input = if (sample_rate as i32) == self.native_sr {
            samples.to_vec()
        } else if let Some(r) = self.in_resampler.as_mut() {
            resample(r, samples)
        } else {
            samples.to_vec()
        };

        // Denoise at native rate.
        let denoised = self.denoiser.run(&native_input, self.native_sr);
        let mut enhanced = denoised.samples;

        // native_sr → caller_sr
        if (sample_rate as i32) != self.native_sr {
            if let Some(r) = self.out_resampler.as_mut() {
                enhanced = resample(r, &enhanced);
            }
        }
        enhanced
    }

    fn name(&self) -> &'static str {
        "sherpa-gtcrn"
    }
}

/// Build a matched pair of resamplers for caller_sr ↔ native_sr.
fn build_resampler_pair(
    native_sr: i32,
    caller_sr: i32,
) -> Result<(FastFixedIn<f32>, FastFixedIn<f32>)> {
    let in_ratio = native_sr as f64 / caller_sr as f64;
    let out_ratio = caller_sr as f64 / native_sr as f64;
    let r_in = FastFixedIn::<f32>::new(in_ratio, 1.0, PolynomialDegree::Septic, 1024, 1)
        .map_err(|e| anyhow!("enhancer in-resampler init: {e}"))?;
    let r_out = FastFixedIn::<f32>::new(out_ratio, 1.0, PolynomialDegree::Septic, 1024, 1)
        .map_err(|e| anyhow!("enhancer out-resampler init: {e}"))?;
    Ok((r_in, r_out))
}

/// Fixed-in resampler call, chunked to the resampler's frame size (1024),
/// padding the final partial block with zeros.
fn resample(resampler: &mut FastFixedIn<f32>, samples: &[f32]) -> Vec<f32> {
    let chunk = 1024usize;
    let mut out = Vec::new();
    let mut idx = 0;
    let mut input_buf: [Vec<f32>; 1] = [Vec::with_capacity(chunk)];
    while idx + chunk <= samples.len() {
        input_buf[0].clear();
        input_buf[0].extend_from_slice(&samples[idx..idx + chunk]);
        if let Ok(processed) = resampler.process(&input_buf, None) {
            out.extend_from_slice(&processed[0]);
        }
        idx += chunk;
    }
    if idx < samples.len() {
        input_buf[0].clear();
        input_buf[0].extend_from_slice(&samples[idx..]);
        input_buf[0].resize(chunk, 0.0);
        if let Ok(processed) = resampler.process(&input_buf, None) {
            out.extend_from_slice(&processed[0]);
        }
    }
    out
}

/// Factory: build the enhancer selected by `config`, or a no-op pass-through.
/// Errors during denoiser construction are caught and downgraded to a no-op
/// with a logged warning, so ASR never breaks because enhancement is missing.
pub fn build_enhancer(config: &AudioEnhancementConfig) -> Box<dyn AudioEnhancer> {
    if !config.enabled {
        return Box::new(NoOpEnhancer);
    }
    match SherpaEnhancer::try_new(config) {
        Ok(e) => Box::new(e),
        Err(e) => {
            eprintln!("[ENHANCE] disabled (load failed): {e}");
            Box::new(NoOpEnhancer)
        }
    }
}
