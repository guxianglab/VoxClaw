//! Moonshine Voice v2 streaming ASR — **evaluation scaffold (not compiled).**
//!
//! This module is intentionally **not** registered in `asr/mod.rs` and is
//! therefore not part of the build. It exists to document the integration plan
//! and reserve the namespace so that enabling Moonshine is a small, localized
//! change once its Mandarin accuracy has been validated.
//!
//! ## Why deferred
//! Moonshine v2 is ONNX-native and has an idiomatic Rust crate (`moonshine-rs`),
//! and its English WER reportedly beats Whisper Large v3 at ~250M params. But
//! its Mandarin CER is **not publicly benchmarked**; SenseVoice and the
//! Zipformer bilingual model were trained Chinese-first. Wiring it in as a hard
//! dependency before measuring Mandarin on our own data would risk a quality
//! regression.
//!
//! ## Activation steps (see also ../../../../EVALUATION.md)
//! 1. Add to `src-tauri/Cargo.toml`:
//!    ```toml
//!    moonshine-rs = { version = "0.1", optional = true }
//!    [features]
//!    moonshine = ["dep:moonshine-rs"]
//!    ```
//! 2. Add `pub mod moonshine;` to `src-tauri/src/asr/mod.rs`.
//! 3. Implement `MoonshineProvider` in `provider.rs` here, mirroring
//!    `crate::asr::zipformer::ZipformerProvider` (same `AsrProvider` /
//!    `AsrSession` surface).
//! 4. Add `MoonshineStreaming` to `AsrProviderKind` + `MoonshineConfig` in
//!    `storage.rs`, plus a `download_moonshine_model` command and a settings
//!    panel.
//! 5. **Gate on evaluation:** Mandarin CER must match/beat the Zipformer
//!    bilingual model before promoting out from behind the `moonshine` feature
//!    flag.
//!
//! The model family to target is `moonshine-ai/moonshine` (Medium Streaming)
//! via ONNX. Confirm tokeniser/vocab handling and the streaming chunk protocol
//! against the `moonshine-rs` crate docs before implementing.

pub mod provider;
