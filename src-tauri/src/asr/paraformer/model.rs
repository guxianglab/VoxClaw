//! Paraformer model file management.
//!
//! A streaming Paraformer model directory contains:
//!   - `encoder*.onnx` (we prefer `*.int8.onnx` when present)
//!   - `decoder*.onnx`
//!   - `tokens.txt`
//!
//! The exact filenames depend on the pretrained release (fp32 vs int8); we
//! search by suffix and prefer the smaller int8 variants for a desktop app.

use std::path::{Path, PathBuf};

/// The tokens filename is always `tokens.txt`.
pub const TOKENS_FILE: &str = "tokens.txt";

/// Find the best `*<keyword>*.onnx` file in `dir`, preferring the int8 variant
/// (smaller, faster on CPU with negligible accuracy loss for VAD-chunked
/// inference). Returns the first match, or `None`.
fn find_onnx(dir: &Path, keyword: &str) -> Option<PathBuf> {
    let entries: Vec<_> = std::fs::read_dir(dir).ok()?.flatten().collect();
    let lower_names: Vec<String> = entries
        .iter()
        .map(|e| e.file_name().to_string_lossy().to_lowercase())
        .collect();

    // Prefer a name containing both `keyword` and `int8`.
    let int8 = entries.iter().zip(lower_names.iter()).find(|(_, name)| {
        name.contains(keyword) && name.contains("int8") && name.ends_with(".onnx")
    });
    if let Some((entry, _)) = int8 {
        return Some(entry.path());
    }

    // Otherwise the first `*keyword*.onnx`.
    entries
        .iter()
        .zip(lower_names.iter())
        .find(|(_, name)| name.contains(keyword) && name.ends_with(".onnx"))
        .map(|(entry, _)| entry.path())
}

/// Resolve the encoder .onnx path inside a model directory (prefers int8).
pub fn encoder_file(dir: &Path) -> Option<PathBuf> {
    find_onnx(dir, "encoder")
}

/// Resolve the decoder .onnx path inside a model directory (prefers int8).
pub fn decoder_file(dir: &Path) -> Option<PathBuf> {
    find_onnx(dir, "decoder")
}

/// Resolve the tokens.txt path inside a model directory.
pub fn tokens_file(dir: &Path) -> PathBuf {
    dir.join(TOKENS_FILE)
}

/// True iff the model directory contains encoder + decoder .onnx and tokens.txt.
pub fn is_present(dir: &Path) -> bool {
    encoder_file(dir).is_some() && decoder_file(dir).is_some() && tokens_file(dir).is_file()
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
        assert_eq!(missing.len(), 3);
    }

    #[test]
    fn is_present_false_for_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(!is_present(tmp.path()));
    }

    #[test]
    fn prefers_int8_variant() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        std::fs::write(d.join("encoder.onnx"), b"x").unwrap();
        std::fs::write(d.join("encoder.int8.onnx"), b"x").unwrap();
        std::fs::write(d.join("decoder.int8.onnx"), b"x").unwrap();
        std::fs::write(d.join("tokens.txt"), b"x").unwrap();

        let enc = encoder_file(d).unwrap();
        assert!(
            enc.to_string_lossy().contains("int8"),
            "should prefer int8: {}",
            enc.display()
        );
        assert!(is_present(d));
    }
}
