//! SenseVoiceSmall provider — non-streaming model used in a "fake-stream"
//! fashion: audio is buffered, the inference runs once on `finish_and_wait`,
//! and the final transcript is also re-emitted as a partial right before
//! returning so the existing `stream_update` UI flow still sees text.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;

use anyhow::{anyhow, Result};
use ndarray::{Array1, Array3};
use ort::{
    session::{builder::GraphOptimizationLevel, Session},
    value::Tensor,
};
use rubato::{FastFixedIn, PolynomialDegree, Resampler};

use crate::storage::SenseVoiceOnnxConfig;

use super::super::{AsrCapabilities, AsrProvider, AsrSession, AsrStreamParams};
use super::decode::{ctc_greedy, TokenVocab};
use super::fbank::{apply_cmvn_lfr, apply_lfr, parse_cmvn, FbankExtractor, FEAT_DIM};
use super::model;

const TARGET_SR: f32 = 16_000.0;
static ORT_RUNTIME_INIT: OnceLock<Result<(), String>> = OnceLock::new();

const ONNXRUNTIME_ENV_VARS: &[&str] = &[
    "SONICCLAW_ONNXRUNTIME_DLL",
    "ONNXRUNTIME_DLL",
    "ORT_DYLIB_PATH",
];

// The exported SenseVoice ONNX model does not take full vocab token IDs here.
// It expects the compact control indices used by the original PyTorch model:
// auto=0, zh=3, en=4, yue=7, ja=11, ko=12, nospeech=13, withitn=14, woitn=15.
fn language_control_id(code: &str) -> Option<i32> {
    match code {
        "auto" | "" => Some(0),
        "zh" => Some(3),
        "en" => Some(4),
        "yue" => Some(7),
        "ja" => Some(11),
        "ko" => Some(12),
        "nospeech" => Some(13),
        _ => None,
    }
}

const TEXT_NORM_WITH_ITN_ID: i32 = 14;

fn ensure_ort_runtime() -> Result<()> {
    match ORT_RUNTIME_INIT.get_or_init(|| init_ort_runtime().map_err(|err| err.to_string())) {
        Ok(()) => Ok(()),
        Err(err) => Err(anyhow!(err.clone())),
    }
}

fn init_ort_runtime() -> Result<()> {
    let dll_path = find_onnxruntime_dll().ok_or_else(|| {
        anyhow!(
            "unable to locate onnxruntime.dll; set SONICCLAW_ONNXRUNTIME_DLL to a compatible DLL path or place it next to the executable"
        )
    })?;

    ort::init_from(&dll_path)
        .map_err(|err| anyhow!("load ONNX Runtime from {}: {err}", dll_path.display()))?
        .commit();

    Ok(())
}

fn find_onnxruntime_dll() -> Option<PathBuf> {
    for key in ONNXRUNTIME_ENV_VARS {
        if let Some(path) = std::env::var_os(key).map(PathBuf::from) {
            if path.is_file() {
                return Some(path);
            }
        }
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for candidate in [dir.join("onnxruntime.dll"), dir.join("lib").join("onnxruntime.dll")] {
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }

    find_python_onnxruntime_dll()
}

fn find_python_onnxruntime_dll() -> Option<PathBuf> {
    let local_app_data = std::env::var_os("LOCALAPPDATA")?;
    let python_root = PathBuf::from(local_app_data).join("Programs").join("Python");
    let mut candidates = Vec::new();

    if let Ok(entries) = std::fs::read_dir(python_root) {
        for entry in entries.flatten() {
            let candidate = entry
                .path()
                .join("Lib")
                .join("site-packages")
                .join("onnxruntime")
                .join("capi")
                .join("onnxruntime.dll");
            if candidate.is_file() {
                candidates.push(candidate);
            }
        }
    }

    candidates.sort();
    candidates.pop()
}

pub struct SenseVoiceProvider {
    inner: Arc<Inner>,
}

struct Inner {
    session: Mutex<Session>,
    vocab: TokenVocab,
    fbank: FbankExtractor,
    neg_mean: Vec<f32>,
    inv_stddev: Vec<f32>,
    language_id: i32,
    blank_id: u32,
}

impl SenseVoiceProvider {
    pub fn try_new(config: &SenseVoiceOnnxConfig) -> Result<Self> {
        ensure_ort_runtime()?;

        let dir = PathBuf::from(&config.model_dir);
        if !model::is_present(&dir) {
            return Err(anyhow!(
                "SenseVoice model files missing at {} (use the Settings → 模型 → 下载模型 button)",
                dir.display()
            ));
        }

        let session = Session::builder()
            .map_err(|e| anyhow!("ort builder: {e}"))?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|e| anyhow!("ort opt level: {e}"))?
            .with_intra_threads(num_cpus_for_inference())
            .map_err(|e| anyhow!("ort threads: {e}"))?
            .commit_from_file(model::model_file(&dir))
            .map_err(|e| anyhow!("ort load model: {e}"))?;

        let vocab = TokenVocab::load(&model::tokens_file(&dir))?;

        let mvn_text = std::fs::read_to_string(dir.join("am.mvn"))
            .map_err(|e| anyhow!("read am.mvn: {e}"))?;
        let (neg_mean, inv_stddev) = parse_cmvn(&mvn_text)?;
        if neg_mean.len() != FEAT_DIM {
            return Err(anyhow!(
                "am.mvn dim {} != expected {}",
                neg_mean.len(),
                FEAT_DIM
            ));
        }

        let language_id = language_control_id(config.language.as_str())
            .ok_or_else(|| anyhow!("unsupported SenseVoice language: {}", config.language))?;

        Ok(Self {
            inner: Arc::new(Inner {
                session: Mutex::new(session),
                vocab,
                fbank: FbankExtractor::new(),
                neg_mean,
                inv_stddev,
                language_id,
                blank_id: 0,
            }),
        })
    }

    pub fn warmup(&self) {
        // Touch the session lock so the first real call doesn't pay the
        // contention cost.
        let _guard = self.inner.session.lock();
        drop(_guard);
    }
}

impl AsrProvider for SenseVoiceProvider {
    fn name(&self) -> &'static str {
        "sensevoice_onnx"
    }

    fn capabilities(&self) -> AsrCapabilities {
        AsrCapabilities {
            streaming: false,
            offline: true,
            languages: vec![
                "auto".into(),
                "zh".into(),
                "en".into(),
                "ja".into(),
                "ko".into(),
                "yue".into(),
            ],
            supports_diarization: false,
        }
    }

    fn start_streaming(&self, params: AsrStreamParams) -> Result<Box<dyn AsrSession>> {
        let AsrStreamParams {
            audio_rx,
            sample_rate,
            on_update,
        } = params;

        let inner = self.inner.clone();
        let buffer = Arc::new(Mutex::new(Vec::<f32>::new()));
        let buffer_for_thread = buffer.clone();

        // Buffer-collector thread: drains audio chunks until the sender is
        // dropped (caller closes the session).
        let collector = thread::spawn(move || {
            while let Ok(chunk) = audio_rx.recv() {
                if let Ok(mut buf) = buffer_for_thread.lock() {
                    buf.extend_from_slice(&chunk);
                }
            }
        });

        Ok(Box::new(SenseVoiceSession {
            inner,
            buffer,
            collector: Some(collector),
            sample_rate,
            on_update,
        }))
    }
}

pub struct SenseVoiceSession {
    inner: Arc<Inner>,
    buffer: Arc<Mutex<Vec<f32>>>,
    collector: Option<thread::JoinHandle<()>>,
    sample_rate: u32,
    on_update: Box<dyn Fn(String) + Send + Sync + 'static>,
}

impl AsrSession for SenseVoiceSession {
    fn finish_and_wait(mut self: Box<Self>) -> Result<String> {
        // Wait for the collector to finish draining (it exits when the audio
        // sender is dropped).
        if let Some(h) = self.collector.take() {
            let _ = h.join();
        }

        let samples = {
            let mut guard = self
                .buffer
                .lock()
                .map_err(|_| anyhow!("audio buffer poisoned"))?;
            std::mem::take(&mut *guard)
        };
        if samples.is_empty() {
            return Ok(String::new());
        }

        let resampled = if (self.sample_rate as f32 - TARGET_SR).abs() < 1.0 {
            samples
        } else {
            resample_to_16k(&samples, self.sample_rate)?
        };

        let text = run_inference(&self.inner, &resampled)?;
        if !text.is_empty() {
            (self.on_update)(text.clone());
        }
        Ok(text)
    }
}

fn run_inference(inner: &Inner, samples_16k: &[f32]) -> Result<String> {
    // 1. log-mel fbank.
    let frames = inner.fbank.compute(samples_16k);
    if frames.is_empty() {
        return Ok(String::new());
    }

    // 2. LFR stack → CMVN.
    let mut feats = apply_lfr(&frames);
    apply_cmvn_lfr(&mut feats, &inner.neg_mean, &inner.inv_stddev);
    let t = feats.len() / FEAT_DIM;

    // 3. Build ONNX inputs (N=1, T, C=560).
    let speech = Array3::from_shape_vec((1, t, FEAT_DIM), feats)
        .map_err(|e| anyhow!("speech reshape: {e}"))?;
    let speech_lengths = Array1::<i32>::from(vec![t as i32]);
    let language = Array1::<i32>::from(vec![inner.language_id]);
    let text_norm = Array1::<i32>::from(vec![TEXT_NORM_WITH_ITN_ID]);

    let mut session = inner
        .session
        .lock()
        .map_err(|_| anyhow!("ort session poisoned"))?;

    let outputs = session
        .run(ort::inputs![
            "speech" => Tensor::from_array(speech).map_err(|e| anyhow!("speech tensor: {e}"))?,
            "speech_lengths" => Tensor::from_array(speech_lengths).map_err(|e| anyhow!("speech_lengths tensor: {e}"))?,
            "language" => Tensor::from_array(language).map_err(|e| anyhow!("language tensor: {e}"))?,
            "textnorm" => Tensor::from_array(text_norm).map_err(|e| anyhow!("textnorm tensor: {e}"))?,
        ])
        .map_err(|e| anyhow!("ort run: {e}"))?;

    // SenseVoice has subsampling factor 1, so output T == input T.
    let (_name, value) = outputs
        .iter()
        .next()
        .ok_or_else(|| anyhow!("ort: no outputs"))?;
    let logits = value
        .try_extract_array::<f32>()
        .map_err(|e| anyhow!("extract logits: {e}"))?;
    let shape: Vec<usize> = logits.shape().to_vec();
    if shape.len() != 3 {
        return Err(anyhow!("unexpected logits shape: {:?}", shape));
    }
    let t_out = shape[1];
    let vocab_size = shape[2];
    let logits_slice = logits
        .as_slice()
        .ok_or_else(|| anyhow!("logits not contiguous"))?;

    let ids = ctc_greedy(logits_slice, t_out, vocab_size, inner.blank_id);
    Ok(inner.vocab.decode_ids(&ids))
}

fn resample_to_16k(samples: &[f32], from_sr: u32) -> Result<Vec<f32>> {
    let ratio = TARGET_SR as f64 / from_sr as f64;
    let chunk = 1024usize;
    let mut resampler =
        FastFixedIn::<f32>::new(ratio, 1.0, PolynomialDegree::Septic, chunk, 1)
            .map_err(|e| anyhow!("resampler init: {e}"))?;

    let mut out = Vec::with_capacity((samples.len() as f64 * ratio) as usize + chunk);
    let mut input_buf: [Vec<f32>; 1] = [Vec::with_capacity(chunk)];
    let mut idx = 0;
    while idx + chunk <= samples.len() {
        input_buf[0].clear();
        input_buf[0].extend_from_slice(&samples[idx..idx + chunk]);
        let processed = resampler
            .process(&input_buf, None)
            .map_err(|e| anyhow!("resample: {e}"))?;
        out.extend_from_slice(&processed[0]);
        idx += chunk;
    }
    // Pad-and-flush the trailing partial frame.
    if idx < samples.len() {
        input_buf[0].clear();
        input_buf[0].extend_from_slice(&samples[idx..]);
        input_buf[0].resize(chunk, 0.0);
        let processed = resampler
            .process(&input_buf, None)
            .map_err(|e| anyhow!("resample tail: {e}"))?;
        let kept = ((samples.len() - idx) as f64 * ratio) as usize;
        out.extend_from_slice(&processed[0][..kept.min(processed[0].len())]);
    }
    Ok(out)
}

fn num_cpus_for_inference() -> usize {
    std::thread::available_parallelism()
        .map(|n| (n.get() / 2).max(1))
        .unwrap_or(2)
}

#[allow(dead_code)]
pub fn try_load_default_dir(_dir: &Path) {}
