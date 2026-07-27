//! Transport-independent transcription requests, transcripts, lifecycle state, and errors.

use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transcript {
    pub raw_text: String,
    pub final_text: String,
    pub segments: Vec<TranscriptSegment>,
    pub engine: String,
    pub model: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptSegment {
    pub text: String,
    pub start_millis: u64,
    pub end_millis: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TranscriptionOptions {
    pub language: Option<String>,
    pub translate: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AudioSource {
    File(PathBuf),
    UnixFd(i32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptionRequest {
    pub source: AudioSource,
    pub options: TranscriptionOptions,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ModelState {
    Initializing,
    Downloading { progress: f32 },
    Loading,
    Ready,
    Failed { message: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicErrorKind {
    NotReady,
    InvalidOptions,
    InvalidAudio,
    ModelUnavailable,
    ResourceLimit,
    TranscriptionFailed,
}

impl PublicErrorKind {
    pub const fn dbus_name(self) -> &'static str {
        match self {
            Self::NotReady => "com.garntresearch.sophon.NotReady",
            Self::InvalidOptions => "com.garntresearch.sophon.InvalidOptions",
            Self::InvalidAudio => "com.garntresearch.sophon.InvalidAudio",
            Self::ModelUnavailable => "com.garntresearch.sophon.ModelUnavailable",
            Self::ResourceLimit => "com.garntresearch.sophon.ResourceLimit",
            Self::TranscriptionFailed => "com.garntresearch.sophon.TranscriptionFailed",
        }
    }
}

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
}

impl SophonError {
    pub const fn public_kind(&self) -> PublicErrorKind {
        match self {
            Self::NotReady => PublicErrorKind::NotReady,
            Self::InvalidOptions(_) => PublicErrorKind::InvalidOptions,
            Self::InvalidAudio(_) => PublicErrorKind::InvalidAudio,
            Self::ModelUnavailable(_) => PublicErrorKind::ModelUnavailable,
            Self::ResourceLimit(_) => PublicErrorKind::ResourceLimit,
            Self::TranscriptionFailed(_) => PublicErrorKind::TranscriptionFailed,
        }
    }
}
