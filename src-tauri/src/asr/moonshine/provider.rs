//! `MoonshineProvider` — placeholder.
//!
//! See `mod.rs` for the activation plan. This file intentionally has no
//! compilable body yet; it documents the intended shape so the integration is
//! mechanical once Mandarin accuracy is validated.
//!
//! ```ignore
//! // Intended implementation outline (mirrors ZipformerProvider):
//! //
//! // pub struct MoonshineProvider { /* moonshine_rs recognizer handle */ }
//! //
//! // impl MoonshineProvider {
//! //     pub fn try_new(config: &MoonshineConfig) -> Result<Self> { ... }
//! // }
//! //
//! // impl AsrProvider for MoonshineProvider {
//! //     fn name(&self) -> &'static str { "moonshine_streaming" }
//! //     fn capabilities(&self) -> AsrCapabilities {
//! //         AsrCapabilities { streaming: true, offline: true,
//! //             languages: vec!["zh".into(), "en".into()],
//! //             supports_diarization: false }
//! //     }
//! //     fn start_streaming(&self, params: AsrStreamParams) -> Result<Box<dyn AsrSession>> { ... }
//! // }
//! ```
