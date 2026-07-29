//! Transport-independent STT/TTS requests, results, lifecycle state, and errors.

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

#[derive(Debug, Clone, PartialEq)]
pub struct OwnedAudio {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum VoiceIntent {
    Default,
    Named(String),
    Clone {
        reference: OwnedAudio,
        transcript: Option<String>,
    },
    Design(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct TtsRequest {
    pub text: String,
    pub language: Option<String>,
    pub speed: f64,
    pub voice: VoiceIntent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TtsCapabilities {
    pub named_voices: bool,
    pub voice_cloning: bool,
    pub voice_design: bool,
    pub speed_control: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TtsState {
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
    InvalidTtsOptions,
    InvalidReferenceAudio,
    UnsupportedCapability,
    OutputExists,
    OutputFailed,
    SynthesisFailed,
    PlaybackFailed,
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
            Self::InvalidTtsOptions => "com.garntresearch.sophon.InvalidTtsOptions",
            Self::InvalidReferenceAudio => "com.garntresearch.sophon.InvalidReferenceAudio",
            Self::UnsupportedCapability => "com.garntresearch.sophon.UnsupportedCapability",
            Self::OutputExists => "com.garntresearch.sophon.OutputExists",
            Self::OutputFailed => "com.garntresearch.sophon.OutputFailed",
            Self::SynthesisFailed => "com.garntresearch.sophon.SynthesisFailed",
            Self::PlaybackFailed => "com.garntresearch.sophon.PlaybackFailed",
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

impl SophonError {
    pub const fn public_kind(&self) -> PublicErrorKind {
        match self {
            Self::NotReady => PublicErrorKind::NotReady,
            Self::InvalidOptions(_) => PublicErrorKind::InvalidOptions,
            Self::InvalidAudio(_) => PublicErrorKind::InvalidAudio,
            Self::ModelUnavailable(_) => PublicErrorKind::ModelUnavailable,
            Self::ResourceLimit(_) => PublicErrorKind::ResourceLimit,
            Self::TranscriptionFailed(_) => PublicErrorKind::TranscriptionFailed,
            Self::InvalidTtsOptions(_) => PublicErrorKind::InvalidTtsOptions,
            Self::InvalidReferenceAudio(_) => PublicErrorKind::InvalidReferenceAudio,
            Self::UnsupportedCapability(_) => PublicErrorKind::UnsupportedCapability,
            Self::OutputExists(_) => PublicErrorKind::OutputExists,
            Self::OutputFailed(_) => PublicErrorKind::OutputFailed,
            Self::SynthesisFailed(_) => PublicErrorKind::SynthesisFailed,
            Self::PlaybackFailed(_) => PublicErrorKind::PlaybackFailed,
        }
    }
}
