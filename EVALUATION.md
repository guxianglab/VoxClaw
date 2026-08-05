# ASR Engine Evaluation Plan

This document tracks the evaluation of new streaming ASR engines against the
existing baselines. It exists so that future integration work has a clear,
version-controlled checklist rather than living in chat history.

## Current baselines (as of v1.3.0)

| Engine | Stack | Streaming | Role |
|---|---|---|---|
| Volcengine (豆包) | WebSocket cloud | yes | cloud fallback, highest accuracy but needs network + keys |
| SenseVoice-Small | ONNX Runtime (ort 2.0.0-rc.11) | no (VAD-chunked) | offline final-pass with emotion/event tags |
| Streaming Zipformer | sherpa-onnx `OnlineRecognizer` | yes | offline streaming for meetings |
| **Streaming Paraformer** (new, P1) | sherpa-onnx `OnlineRecognizer` | yes | offline streaming, **better Mandarin CER than Zipformer** |

## Candidate engines to evaluate (P2)

### 1. Moonshine Voice v2 (Moonshine Voice / `moonshine-ai/moonshine`)

**Why it's interesting:** natively streaming (Medium Streaming variant),
ONNX-native, ~250M params, has an idiomatic Rust crate `moonshine-rs` (v0.1.3).
Could *replace both* SenseVoice and streaming Zipformer with a single engine.

**Why it's deferred (not wired in this pass):**
- Mandarin CER is **claimed but not benchmarked publicly**. SenseVoice and
  Paraformer were trained Chinese-first; Moonshine is English-led. Swapping
  without a measured Mandarin test on our data risks a regression.
- `moonshine-rs` is newer than sherpa-onnx (v0.1.3 vs 1.13.4); release cadence
  and Windows/Tauri specifics unverified. Adding it as a hard dependency before
  evaluation would risk the build.

**Integration plan (when ready to evaluate):**
1. Add `moonshine-rs = "0.1"` to `src-tauri/Cargo.toml` behind a
   `moonshine` Cargo feature, so the default build is unaffected.
2. Implement `MoonshineProvider` in `src-tauri/src/asr/moonshine/provider.rs`
   following the `ParaformerProvider` shape (same `AsrProvider` /
   `AsrSession` contract).
3. Add `MoonshineStreaming` to `AsrProviderKind` + a `MoonshineConfig` in
   `storage.rs`, mirroring the Paraformer config.
4. Wire a `download_moonshine_model` command + settings UI panel.
5. **Gate on this evaluation result:** run Mandarin CER on ≥20 minutes of
   representative meeting/dictation audio. Only promote out from behind the
   feature flag if Moonshine matches or beats Paraformer on Mandarin.

**Stubs in place:** `src-tauri/src/asr/moonshine/mod.rs` is reserved but not
compiled (no `mod moonshine;` in `asr/mod.rs` yet). Uncomment when evaluating.

### 2. Qwen3-ASR — WATCH, not ready
Best Chinese quality (0.6B/1.7B, 52 languages + 22 dialects), but streaming
inference needs a vLLM backend. sherpa-onnx only covers the **offline** path
today. Revisit when sherpa-onnx adds streaming Qwen3-ASR or a CPU-INT8 ONNX
streaming export lands.

### 3. Fun-ASR-Nano-2512 — WATCH, not ready
CER 4.16%, 31 languages + 7 dialect groups, CPU RTF 0.05–0.1. Local Rust/ONNX
packaging not turnkey (primary path is vLLM/Xinference). Revisit in 3–6 months.

## Speech-enhancement pre-filter (P2 — scaffolded)

`src-tauri/src/audio_enhancement.rs` provides:
- `AudioEnhancer` trait (chunk-level transform before ASR)
- `NoOpEnhancer` (zero-cost pass-through, the default)
- `SherpaEnhancer` wrapping `sherpa_onnx::OnlineSpeechDenoiser` (GTCRN /
  DPDFNet), with automatic caller-rate ↔ native-rate resampling.

Driven by `AppConfig.audio_enhancement` (`enabled`, `gtcrn_model`,
`dpdfnet_model`). The factory `build_enhancer()` degrades to no-op on any load
error so ASR never breaks. **Not yet wired into the capture → ASR path** —
enable by inserting `enhancer.enhance(&chunk, sr)` in
`meeting/audio.rs` (≈ line 320/338) and `audio.rs` (≈ line 492) once a GTCRN
model path is configured and A/B-tested.

### GTCRN model
- Repo: `k2-fsa/sherpa-onnx` ships `gtcrn_simple.onnx` under the
  `speech-enhancement-models` release assets.
- Native rate: 16 kHz (matches the ASR input domain, so resampling is a no-op
  in the common case).
