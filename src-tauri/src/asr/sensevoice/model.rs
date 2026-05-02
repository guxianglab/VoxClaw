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
