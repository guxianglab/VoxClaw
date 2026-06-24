//! sherpa-onnx Streaming Zipformer ASR engine.
//!
//! This is a genuinely *streaming* engine: audio is fed chunk-by-chunk to the
//! transducer, which maintains cross-chunk acoustic state internally. There is
//! no "segment boundary loses context" problem — the model hears a continuous
//! stream and outputs incrementally. This makes it ideal for meeting mode.
//!
//! The engine has a built-in endpoint detector (`enable_endpoint`) that signals
//! sentence boundaries, so no separate VAD is needed.

pub mod download;
pub mod model;
pub mod provider;

pub use provider::ZipformerProvider;
