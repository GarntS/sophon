//! Transport-independent text-to-speech values.

use crate::audio::OwnedAudio;

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
pub enum TtsStreamControl {
    Continue,
    Cancel,
}

#[derive(Debug)]
pub enum TtsStreamEvent {
    Format { sample_rate: u32 },
    Chunk { samples: Vec<f32> },
    Terminal(Result<(), crate::error::SophonError>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TtsCapabilities {
    pub named_voices: bool,
    pub voice_cloning: bool,
    pub voice_design: bool,
    pub speed_control: bool,
}
