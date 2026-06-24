//! SenseVoice model file constants and on-disk layout.

use std::path::{Path, PathBuf};

/// All required files in a SenseVoiceSmall-onnx model directory, with their
/// canonical sizes (used for sanity-checking downloads). Sizes are taken from
/// `https://www.modelscope.cn/api/v1/models/iic/SenseVoiceSmall-onnx/repo/files`.
///
/// We download the **quantised** model by default — it's ~234 MB instead of
/// ~900 MB and the accuracy difference for short utterances is negligible.
pub const REQUIRED_FILES: &[(&str, u64)] = &[
    ("model_quant.onnx", 241_216_270),
    ("tokens.json", 352_064),
    ("config.yaml", 1_855),
    ("am.mvn", 11_203),
    ("configuration.json", 56),
    // NOTE: chn_jpn_yue_eng_ko_spectok.bpe.model was removed from the upstream
    // ModelScope repo and is no longer required for ONNX inference.
];

/// Returns true iff every required file exists in `dir` and `model_quant.onnx`
/// is roughly the expected size (we tolerate ±1% to absorb any future
/// requantisation).
pub fn is_present(dir: &Path) -> bool {
    if !dir.is_dir() {
        return false;
    }
    for (name, expected_size) in REQUIRED_FILES {
        let path = dir.join(name);
        let Ok(meta) = std::fs::metadata(&path) else {
            return false;
        };
        if !meta.is_file() {
            return false;
        }
        if *expected_size > 0 {
            let actual = meta.len();
            let lo = expected_size - expected_size / 100;
            let hi = expected_size + expected_size / 100;
            if actual < lo || actual > hi {
                return false;
            }
        }
    }
    true
}

pub fn model_file(dir: &Path) -> PathBuf {
    dir.join("model_quant.onnx")
}

pub fn tokens_file(dir: &Path) -> PathBuf {
    dir.join("tokens.json")
}

// --- VAD model ---------------------------------------------------------------
// The Silero VAD model lives in a `vad/` subdirectory of a SenseVoice model
// directory so it shares the model_dir lifetime and download UI.

/// The VAD model filename inside a SenseVoice model directory's `vad/` subdir.
pub const VAD_MODEL_FILE: &str = "silero_vad.onnx";

/// Default subdirectory under a SenseVoice model dir holding the VAD model.
pub fn vad_subdir(sensevoice_dir: &Path) -> PathBuf {
    sensevoice_dir.join("vad")
}

pub fn vad_model_file(sensevoice_dir: &Path) -> PathBuf {
    vad_subdir(sensevoice_dir).join(VAD_MODEL_FILE)
}

/// True iff the VAD model exists and is non-trivially sized (> 100 KB). The
/// sherpa-onnx silero_vad.onnx is ~640 KB, so the threshold excludes only
/// empty/partial/corrupt files.
pub fn is_vad_present(sensevoice_dir: &Path) -> bool {
    let path = vad_model_file(sensevoice_dir);
    match std::fs::metadata(&path) {
        Ok(meta) => meta.is_file() && meta.len() > 100_000,
        Err(_) => false,
    }
}
