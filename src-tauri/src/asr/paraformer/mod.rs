//! sherpa-onnx Streaming Paraformer ASR engine.
//!
//! Paraformer is Alibaba/FunAudioLLM's non-autoregressive streaming ASR. It
//! uses the *same* `OnlineRecognizer` runtime as streaming Zipformer — the only
//! difference is the model-config branch: `OnlineParaformerModelConfig`
//! (encoder + decoder) instead of the transducer triad. This lets it plug into
//! the existing `AsrProvider` abstraction with no special-cased session loop.
//!
//! On Mandarin CER, streaming Paraformer generally outperforms streaming
//! Zipformer (it was trained on much more Chinese data), so it is the
//! recommended offline-streaming engine for zh+en meetings.

pub mod download;
pub mod model;
pub mod provider;

pub use provider::ParaformerProvider;
