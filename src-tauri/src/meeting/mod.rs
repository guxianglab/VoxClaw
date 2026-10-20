//! Meeting mode — long-form recording with live transcription and an LLM
//! summary at the end.
//!
//! Independent from dictation: its own audio capture (microphone mixed with
//! system audio via WASAPI loopback), its own streaming ASR session on the
//! currently selected engine, controlled by explicit start/stop commands.

pub mod audio;
pub mod loopback;
pub mod session;
pub mod summary;
