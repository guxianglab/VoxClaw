//! Token vocab loading + CTC greedy decoding for SenseVoice.

use std::collections::HashSet;
use std::path::Path;

use anyhow::{anyhow, Result};
use serde_json::Value;

/// Loaded token table (id → printable string). Index 0..=N matches the model
/// output vocabulary directly.
#[derive(Clone)]
pub struct TokenVocab {
    tokens: Vec<String>,
    /// IDs that are special meta-tags (`<|zh|>`, `<|NEUTRAL|>`, ...) and must
    /// be stripped from the final transcript.
    special_ids: HashSet<u32>,
    /// `▁` (U+2581) marks word boundaries in SentencePiece — convert to space.
    sp_underline: char,
}

impl TokenVocab {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| anyhow!("read {} failed: {e}", path.display()))?;
        let val: Value = serde_json::from_str(&text)
            .map_err(|e| anyhow!("parse tokens.json failed: {e}"))?;

        // SenseVoice ships either a flat array of strings or an array of
        // `[token, score]` pairs. Handle both.
        let arr = val
            .as_array()
            .ok_or_else(|| anyhow!("tokens.json must be a JSON array"))?;
        let mut tokens = Vec::with_capacity(arr.len());
        for item in arr {
            let tok = match item {
                Value::String(s) => s.clone(),
                Value::Array(pair) if !pair.is_empty() => pair[0]
                    .as_str()
                    .ok_or_else(|| anyhow!("tokens.json: pair[0] not a string"))?
                    .to_string(),
                _ => return Err(anyhow!("tokens.json: unexpected entry shape")),
            };
            tokens.push(tok);
        }

        let mut special_ids = HashSet::new();
        for (i, t) in tokens.iter().enumerate() {
            if is_special(t) {
                special_ids.insert(i as u32);
            }
        }

        Ok(Self {
            tokens,
            special_ids,
            sp_underline: '\u{2581}',
        })
    }

    pub fn len(&self) -> usize {
        self.tokens.len()
    }

    pub fn token_id(&self, token: &str) -> Option<u32> {
        self.tokens
            .iter()
            .position(|candidate| candidate == token)
            .map(|index| index as u32)
    }

    /// Decode a greedy ID sequence (already CTC-collapsed) into a string.
    pub fn decode_ids(&self, ids: &[u32]) -> String {
        let mut out = String::new();
        for &id in ids {
            if self.special_ids.contains(&id) {
                continue;
            }
            let Some(tok) = self.tokens.get(id as usize) else {
                continue;
            };
            // Skip blank-style tokens that escape the metadata heuristic.
            if tok.is_empty() || tok == "<blank>" || tok == "<unk>" || tok == "<s>" || tok == "</s>" {
                continue;
            }
            for ch in tok.chars() {
                if ch == self.sp_underline {
                    if !out.is_empty() && !out.ends_with(' ') {
                        out.push(' ');
                    }
                } else {
                    out.push(ch);
                }
            }
        }
        out.trim().to_string()
    }
}

/// Anything wrapped in `<|...|>` (language tag, emotion, event, itn flag…).
fn is_special(token: &str) -> bool {
    let trimmed = token.trim();
    trimmed.starts_with("<|") && trimmed.ends_with("|>")
}

/// CTC greedy decode: argmax per frame, collapse repeats, drop blanks.
pub fn ctc_greedy(logits: &[f32], n_frames: usize, vocab_size: usize, blank_id: u32) -> Vec<u32> {
    debug_assert_eq!(logits.len(), n_frames * vocab_size);
    let mut out = Vec::with_capacity(n_frames);
    let mut prev: i64 = -1;
    for t in 0..n_frames {
        let row = &logits[t * vocab_size..(t + 1) * vocab_size];
        let mut best = 0usize;
        let mut best_v = row[0];
        for (i, &v) in row.iter().enumerate().skip(1) {
            if v > best_v {
                best_v = v;
                best = i;
            }
        }
        let id = best as u32;
        if id != blank_id && id as i64 != prev {
            out.push(id);
        }
        prev = id as i64;
    }
    out
}
