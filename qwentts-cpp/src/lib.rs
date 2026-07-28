//! A safe, stateful wrapper around the qwentts.cpp C ABI.
//!
//! The selected Cargo backend feature builds the vendored qwentts.cpp source.
//! The crate deliberately exposes no streaming or provider-neutral abstraction;
//! it owns one loaded Qwen engine.

mod raw;

use std::{
    ffi::{CStr, CString, NulError},
    path::Path,
    ptr::NonNull,
};

use thiserror::Error;

/// Errors returned by the safe qwentts.cpp API.
#[derive(Debug, Error)]
pub enum QwenTtsError {
    #[error("{field} contains an interior NUL byte")]
    InteriorNul { field: &'static str },
    #[error("text must not be empty")]
    EmptyText,
    #[error("a model must be loaded before {operation}")]
    ModelNotLoaded { operation: &'static str },
    #[error("native initialization failed{diagnostic}")]
    Initialization { diagnostic: String },
    #[error("native {operation} failed with status {status}{diagnostic}")]
    Native {
        operation: &'static str,
        status: i32,
        diagnostic: String,
    },
    #[error("WAV output failed: {0}")]
    Wav(#[from] hound::Error),
}

impl QwenTtsError {
    fn native(operation: &'static str, status: i32) -> Self {
        Self::Native {
            operation,
            status,
            diagnostic: native_diagnostic(),
        }
    }
}

fn native_diagnostic() -> String {
    // SAFETY: qwen returns either null or a thread-local NUL-terminated string.
    unsafe {
        let message = raw::qt_last_error();
        if message.is_null() {
            String::new()
        } else {
            let message = CStr::from_ptr(message).to_string_lossy();
            if message.is_empty() {
                String::new()
            } else {
                format!(": {message}")
            }
        }
    }
}

fn c_string(value: &str, field: &'static str) -> Result<CString, QwenTtsError> {
    CString::new(value).map_err(|_: NulError| QwenTtsError::InteriorNul { field })
}

fn path_string(path: &Path, field: &'static str) -> Result<CString, QwenTtsError> {
    c_string(&path.to_string_lossy(), field)
}

/// Native model-loading settings.
#[derive(Debug, Clone, Copy)]
pub struct ModelOptions {
    pub use_flash_attention: bool,
    pub clamp_fp16: bool,
    pub max_batch: u32,
    pub codec_chunk_seconds: f32,
}

impl Default for ModelOptions {
    fn default() -> Self {
        Self {
            use_flash_attention: true,
            clamp_fp16: false,
            max_batch: 1,
            codec_chunk_seconds: 24.0,
        }
    }
}

/// A language hint accepted by qwentts.cpp.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Auto,
    English,
    Chinese,
    Japanese,
    Korean,
    German,
    French,
    Russian,
    Portuguese,
    Spanish,
    Italian,
}

impl Language {
    fn as_str(self) -> Option<&'static str> {
        match self {
            Self::Auto => None,
            Self::English => Some("english"),
            Self::Chinese => Some("chinese"),
            Self::Japanese => Some("japanese"),
            Self::Korean => Some("korean"),
            Self::German => Some("german"),
            Self::French => Some("french"),
            Self::Russian => Some("russian"),
            Self::Portuguese => Some("portuguese"),
            Self::Spanish => Some("spanish"),
            Self::Italian => Some("italian"),
        }
    }
}

/// Sampling controls for a synthesis request.
#[derive(Debug, Clone)]
pub struct SamplingOptions {
    pub seed: Option<i64>,
    pub max_new_tokens: u32,
    pub temperature: f32,
    pub top_k: u32,
    pub top_p: f32,
    pub repetition_penalty: f32,
}

impl Default for SamplingOptions {
    fn default() -> Self {
        Self {
            seed: None,
            max_new_tokens: 2048,
            temperature: 0.9,
            top_k: 50,
            top_p: 1.0,
            repetition_penalty: 1.05,
        }
    }
}

/// The Qwen-specific voice intent for a synthesis request.
#[derive(Debug)]
pub enum Voice<'a> {
    Default,
    Named(&'a str),
    Clone(&'a VoiceReference),
    CloneWithTranscript {
        reference: &'a VoiceReference,
        transcript: &'a str,
    },
    Design(&'a str),
}

impl Default for Voice<'_> {
    fn default() -> Self {
        Self::Default
    }
}

/// Options for one buffered synthesis operation.
#[derive(Debug)]
pub struct SynthesisOptions<'a> {
    pub language: Language,
    pub sampling: SamplingOptions,
    pub voice: Voice<'a>,
}

impl Default for SynthesisOptions<'_> {
    fn default() -> Self {
        Self {
            language: Language::Auto,
            sampling: SamplingOptions::default(),
            voice: Voice::Default,
        }
    }
}

/// Rust-owned audio produced by a synthesis call.
#[derive(Debug, Clone, PartialEq)]
pub struct SynthesisResult {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
}

impl SynthesisResult {
    pub fn duration_secs(&self) -> f64 {
        if self.sample_rate == 0 {
            0.0
        } else {
            self.samples.len() as f64 / f64::from(self.sample_rate)
        }
    }

    pub fn write_wav(&self, path: impl AsRef<Path>) -> Result<(), QwenTtsError> {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: self.sample_rate,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut writer = hound::WavWriter::create(path, spec)?;
        for sample in &self.samples {
            writer.write_sample(*sample)?;
        }
        writer.finalize()?;
        Ok(())
    }
}

/// A native pre-encoded clone reference. It frees its native buffers on drop.
pub struct VoiceReference {
    raw: raw::qt_voice_ref,
}

impl std::fmt::Debug for VoiceReference {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("VoiceReference(..)")
    }
}

impl Drop for VoiceReference {
    fn drop(&mut self) {
        // SAFETY: raw was zeroed or initialized by qt_extract_voice_ref and is owned here.
        unsafe { raw::qt_voice_ref_free(&mut self.raw) }
    }
}

/// A mutable qwentts.cpp engine. It starts unloaded.
pub struct QwenTtsEngine {
    context: Option<NonNull<raw::qt_context>>,
}

impl Default for QwenTtsEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl QwenTtsEngine {
    pub fn new() -> Self {
        Self { context: None }
    }

    pub fn is_loaded(&self) -> bool {
        self.context.is_some()
    }

    pub fn load_model(
        &mut self,
        talker_path: impl AsRef<Path>,
        codec_path: impl AsRef<Path>,
    ) -> Result<(), QwenTtsError> {
        self.load_model_with_options(talker_path, codec_path, ModelOptions::default())
    }

    pub fn load_model_with_options(
        &mut self,
        talker_path: impl AsRef<Path>,
        codec_path: impl AsRef<Path>,
        options: ModelOptions,
    ) -> Result<(), QwenTtsError> {
        let talker = path_string(talker_path.as_ref(), "talker model path")?;
        let codec = path_string(codec_path.as_ref(), "codec model path")?;
        // SAFETY: native defaults initialize every ABI field; C strings outlive qt_init.
        let context = unsafe {
            let mut params = std::mem::zeroed();
            raw::qt_init_default_params(&mut params);
            params.talker_path = talker.as_ptr();
            params.codec_path = codec.as_ptr();
            params.use_fa = options.use_flash_attention;
            params.clamp_fp16 = options.clamp_fp16;
            params.max_batch = options.max_batch as i32;
            params.codec_chunk_sec = options.codec_chunk_seconds;
            NonNull::new(raw::qt_init(&params))
        }
        .ok_or_else(|| QwenTtsError::Initialization {
            diagnostic: native_diagnostic(),
        })?;
        self.unload_model();
        self.context = Some(context);
        Ok(())
    }

    pub fn unload_model(&mut self) {
        if let Some(context) = self.context.take() {
            // SAFETY: context is exclusively owned by this engine.
            unsafe { raw::qt_free(context.as_ptr()) }
        }
    }

    fn context(&self, operation: &'static str) -> Result<NonNull<raw::qt_context>, QwenTtsError> {
        self.context
            .ok_or(QwenTtsError::ModelNotLoaded { operation })
    }

    pub fn synthesize(
        &mut self,
        text: &str,
        options: Option<SynthesisOptions<'_>>,
    ) -> Result<SynthesisResult, QwenTtsError> {
        if text.is_empty() {
            return Err(QwenTtsError::EmptyText);
        }
        let context = self.context("synthesis")?;
        let text = c_string(text, "text")?;
        let options = options.unwrap_or_default();
        let language = options
            .language
            .as_str()
            .map(|value| c_string(value, "language"))
            .transpose()?;
        let (speaker, instruction, transcript, reference) = match options.voice {
            Voice::Default => (None, None, None, None),
            Voice::Named(name) => (Some(c_string(name, "speaker")?), None, None, None),
            Voice::Clone(reference) => (None, None, None, Some(reference)),
            Voice::CloneWithTranscript {
                reference,
                transcript,
            } => (
                None,
                None,
                Some(c_string(transcript, "transcript")?),
                Some(reference),
            ),
            Voice::Design(instruction) => (
                None,
                Some(c_string(instruction, "instruction")?),
                None,
                None,
            ),
        };
        // SAFETY: pointers refer to the local C strings/reference for this synchronous call.
        unsafe {
            let mut params = std::mem::zeroed();
            raw::qt_tts_default_params(&mut params);
            params.text = text.as_ptr();
            params.lang = language
                .as_ref()
                .map_or(std::ptr::null(), |value| value.as_ptr());
            params.speaker = speaker
                .as_ref()
                .map_or(std::ptr::null(), |value| value.as_ptr());
            params.instruct = instruction
                .as_ref()
                .map_or(std::ptr::null(), |value| value.as_ptr());
            params.ref_text = transcript
                .as_ref()
                .map_or(std::ptr::null(), |value| value.as_ptr());
            params.seed = options.sampling.seed.unwrap_or(-1);
            params.max_new_tokens = options.sampling.max_new_tokens as i32;
            params.temperature = options.sampling.temperature;
            params.top_k = options.sampling.top_k as i32;
            params.top_p = options.sampling.top_p;
            params.repetition_penalty = options.sampling.repetition_penalty;
            if let Some(reference) = reference {
                params.ref_spk_emb = reference.raw.ref_spk_emb;
                params.ref_spk_dim = reference.raw.ref_spk_dim;
                params.ref_codes = reference.raw.ref_codes;
                params.ref_T = reference.raw.ref_T;
            }
            let mut audio: raw::qt_audio = std::mem::zeroed();
            let status = raw::qt_synthesize(context.as_ptr(), &params, &mut audio) as i32;
            if status != 0 {
                raw::qt_audio_free(&mut audio);
                return Err(QwenTtsError::native("synthesis", status));
            }
            let samples = if audio.samples.is_null() || audio.n_samples <= 0 {
                Vec::new()
            } else {
                std::slice::from_raw_parts(audio.samples, audio.n_samples as usize).to_vec()
            };
            let sample_rate = audio.sample_rate.max(0) as u32;
            raw::qt_audio_free(&mut audio);
            Ok(SynthesisResult {
                samples,
                sample_rate,
            })
        }
    }

    pub fn extract_voice_reference(
        &mut self,
        samples_24khz_mono: &[f32],
    ) -> Result<VoiceReference, QwenTtsError> {
        let context = self.context("voice-reference extraction")?;
        // SAFETY: slice stays valid for the synchronous native call and raw is zeroed for native ownership.
        unsafe {
            let mut reference: raw::qt_voice_ref = std::mem::zeroed();
            let status = raw::qt_extract_voice_ref(
                context.as_ptr(),
                samples_24khz_mono.as_ptr(),
                samples_24khz_mono.len() as i32,
                &mut reference,
            ) as i32;
            if status != 0 {
                return Err(QwenTtsError::native("voice-reference extraction", status));
            }
            Ok(VoiceReference { raw: reference })
        }
    }

    pub fn speakers(&self) -> Result<Vec<String>, QwenTtsError> {
        let context = self.context("speaker enumeration")?;
        // SAFETY: speaker pointers remain valid while context is loaded; strings are copied immediately.
        unsafe {
            let count = raw::qt_n_speakers(context.as_ptr()).max(0);
            Ok((0..count)
                .filter_map(|index| {
                    let name = raw::qt_speaker_name(context.as_ptr(), index);
                    (!name.is_null()).then(|| CStr::from_ptr(name).to_string_lossy().into_owned())
                })
                .collect())
        }
    }
}

impl Drop for QwenTtsEngine {
    fn drop(&mut self) {
        self.unload_model();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unloaded_engine_does_not_call_native_synthesis() {
        assert!(matches!(
            QwenTtsEngine::new().synthesize("hello", None),
            Err(QwenTtsError::ModelNotLoaded { .. })
        ));
    }

    #[test]
    fn interior_nuls_are_rejected_before_native_calls() {
        assert!(matches!(
            c_string("bad\0text", "text"),
            Err(QwenTtsError::InteriorNul { field: "text" })
        ));
    }

    #[test]
    fn native_failure_preserves_a_copied_diagnostic() {
        // SAFETY: the ABI explicitly accepts null context/audio arguments and reports an error.
        unsafe {
            let mut reference: raw::qt_voice_ref = std::mem::zeroed();
            let status = raw::qt_extract_voice_ref(
                std::ptr::null_mut(),
                std::ptr::null(),
                0,
                &mut reference,
            ) as i32;
            assert_ne!(status, 0);
            assert!(
                matches!(QwenTtsError::native("voice-reference extraction", status), QwenTtsError::Native { diagnostic, .. } if diagnostic.contains("q, ref_audio_24k or out is NULL"))
            );
            raw::qt_voice_ref_free(&mut reference);
        }
    }

    #[test]
    fn zeroed_voice_reference_can_be_dropped_safely() {
        // SAFETY: qt_voice_ref_free explicitly accepts a zero-initialized reference.
        let reference = VoiceReference {
            raw: unsafe { std::mem::zeroed() },
        };
        drop(reference);
    }

    #[test]
    fn owned_results_calculate_duration_and_write_float_wav() {
        let result = SynthesisResult {
            samples: vec![0.0, 0.25, -0.25],
            sample_rate: 24_000,
        };
        assert_eq!(result.duration_secs(), 3.0 / 24_000.0);
        let file = tempfile::NamedTempFile::new().unwrap();
        result.write_wav(file.path()).unwrap();
        let reader = hound::WavReader::open(file.path()).unwrap();
        assert_eq!(reader.spec().sample_format, hound::SampleFormat::Float);
        assert_eq!(reader.duration(), 3);
    }
}
