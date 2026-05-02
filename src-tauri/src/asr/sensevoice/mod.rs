//! SenseVoiceSmall ONNX provider.
//!
//! Wires together: model file management ([`model`]), audio preprocessing
//! ([`fbank`]), inference + CTC decoding ([`provider`]), and the public
//! [`SenseVoiceProvider`] type implementing [`super::AsrProvider`].

pub mod decode;
pub mod download;
pub mod fbank;
pub mod model;
pub mod provider;

pub use provider::SenseVoiceProvider;
