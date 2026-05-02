//! Meeting mode — long-form recording with ASR transcription.
//!
//! Independent from dictation: separate audio stream lifecycle, separate
//! state machine, controlled by explicit start/stop commands from the
//! frontend (not by the global hotkey).
//!
//! Phase 3 ships **mic-only** capture. WASAPI loopback (system audio) mix
//! is wired in [`audio::MeetingAudioConfig`] but currently records mic only;
//! a follow-up will add the dual-stream mixer.

pub mod audio;
pub mod llm;
pub mod session;
