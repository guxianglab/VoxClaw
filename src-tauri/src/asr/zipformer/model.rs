//! Zipformer model file management.
//!
//! A streaming Zipformer transducer model is a directory containing:
//!   - encoder .onnx
//!   - decoder .onnx
//!   - joiner .onnx
//!   - tokens.txt
//!
//! The exact filenames depend on the pretrained release; we search by suffix.

use std::path::{Path, PathBuf};

/// The three required .onnx file categories in a Zipformer model directory.
pub const REQUIRED_SUFFIXES: &[&str] = &["encoder", "decoder", "joiner"];

/// The tokens filename is always `tokens.txt`.
pub const TOKENS_FILE: &str = "tokens.txt";

/// Find a file in `dir` whose name contains `keyword` and ends with `.onnx`.
/// Returns the first match, or `None`.
fn find_onnx(dir: &Path, keyword: &str) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy().to_lowercase();
        if name.contains(keyword) && name.ends_with(".onnx") {
            return Some(entry.path());
        }
    }
    None
}

/// Resolve the encoder .onnx path inside a model directory.
pub fn encoder_file(dir: &Path) -> Option<PathBuf> {
    find_onnx(dir, "encoder")
}

/// Resolve the decoder .onnx path inside a model directory.
pub fn decoder_file(dir: &Path) -> Option<PathBuf> {
    find_onnx(dir, "decoder")
}

/// Resolve the joiner .onnx path inside a model directory.
pub fn joiner_file(dir: &Path) -> Option<PathBuf> {
    find_onnx(dir, "joiner")
}

/// Resolve the tokens.txt path inside a model directory.
pub fn tokens_file(dir: &Path) -> PathBuf {
    dir.join(TOKENS_FILE)
}

/// True iff the model directory contains all four required files
/// (encoder + decoder + joiner .onnx, and tokens.txt).
pub fn is_present(dir: &Path) -> bool {
    if encoder_file(dir).is_none() {
        return false;
    }
    if decoder_file(dir).is_none() {
        return false;
    }
    if joiner_file(dir).is_none() {
        return false;
    }
    tokens_file(dir).is_file()
}

/// A human-readable diagnostic of which files are missing, for error messages.
pub fn missing_files(dir: &Path) -> Vec<String> {
    let mut missing = Vec::new();
    if encoder_file(dir).is_none() {
        missing.push("encoder*.onnx".to_string());
    }
    if decoder_file(dir).is_none() {
        missing.push("decoder*.onnx".to_string());
    }
    if joiner_file(dir).is_none() {
        missing.push("joiner*.onnx".to_string());
    }
    if !tokens_file(dir).is_file() {
        missing.push(TOKENS_FILE.to_string());
    }
    missing
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_files_lists_all_when_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = missing_files(tmp.path());
        assert_eq!(missing.len(), 4);
    }

    #[test]
    fn is_present_false_for_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(!is_present(tmp.path()));
    }
}
