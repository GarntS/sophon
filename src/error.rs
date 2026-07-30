//! Application-wide errors shared across transports and providers.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum SophonError {
    #[error("model is not ready")]
    NotReady,
    #[error("invalid transcription options: {0}")]
    InvalidOptions(String),
    #[error("invalid audio: {0}")]
    InvalidAudio(String),
    #[error("model unavailable: {0}")]
    ModelUnavailable(String),
    #[error("resource limit exceeded: {0}")]
    ResourceLimit(String),
    #[error("transcription failed: {0}")]
    TranscriptionFailed(String),
    #[error("invalid TTS options: {0}")]
    InvalidTtsOptions(String),
    #[error("invalid reference audio: {0}")]
    InvalidReferenceAudio(String),
    #[error("unsupported TTS capability: {0}")]
    UnsupportedCapability(String),
    #[error("output already exists: {0}")]
    OutputExists(String),
    #[error("output failed: {0}")]
    OutputFailed(String),
    #[error("synthesis failed: {0}")]
    SynthesisFailed(String),
    #[error("playback failed: {0}")]
    PlaybackFailed(String),
}
