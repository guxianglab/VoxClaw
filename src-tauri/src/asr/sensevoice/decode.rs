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
        collapse_excessive_repeats(out.trim())
    }
}

/// Fold runs of the same CJK character longer than 3 to at most 3.
///
/// SenseVoice occasionally emits pathological repetition on short/rapid speech
/// (e.g. "吃早饭" → "吃早早早早早饭"). In Chinese, the same character repeated
/// 4+ times in a row is never legitimate text (legitimate emphasis like
/// "哈哈哈" stays at ≤3). We keep up to 3 and collapse the rest.
fn collapse_excessive_repeats(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return String::new();
    }
    let mut out = String::with_capacity(chars.len());
    let mut run_char = chars[0];
    let mut run_len = 1usize;
    out.push(run_char);

    for &ch in &chars[1..] {
        if ch == run_char {
            run_len += 1;
            // Keep at most 3 consecutive identical CJK characters.
            if is_cjk(run_char) && run_len > 3 {
                continue;
            }
            // Non-CJK (latin/digits): allow legitimate repeats up to 2.
            if !is_cjk(run_char) && run_len > 2 {
                continue;
            }
            out.push(ch);
        } else {
            run_char = ch;
            run_len = 1;
            out.push(ch);
        }
    }
    out
}

/// True for CJK Unified Ideographs (covers Chinese/Japanese/Korean hanzi).
fn is_cjk(ch: char) -> bool {
    matches!(ch,
        '\u{4E00}'..='\u{9FFF}'   // CJK Unified Ideographs
        | '\u{3400}'..='\u{4DBF}' // CJK Extension A
        | '\u{F900}'..='\u{FAFF}' // CJK Compatibility Ideographs
    )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapse_folds_excessive_cjk_repeats() {
        // 5x"早" → folded to 3 (max allowed for CJK).
        assert_eq!(collapse_excessive_repeats("吃早早早早早饭"), "吃早早早饭");
    }

    #[test]
    fn collapse_keeps_legitimate_triple() {
        assert_eq!(collapse_excessive_repeats("哈哈哈"), "哈哈哈");
    }

    #[test]
    fn collapse_keeps_normal_text() {
        assert_eq!(collapse_excessive_repeats("吃早饭去玩"), "吃早饭去玩");
    }

    #[test]
    fn collapse_keeps_distinct_adjacent_chars() {
        // "早早" appearing as a real word stays (run of 2).
        assert_eq!(collapse_excessive_repeats("早早起"), "早早起");
    }
}
