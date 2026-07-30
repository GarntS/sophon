//! Provider-neutral text-to-speech engine contract and scheduling.

pub mod playback;
pub mod service;
pub mod types;

pub use service::TtsService;
pub use types::{TtsCapabilities, TtsRequest, VoiceIntent};

use std::{
    path::{Path, PathBuf},
    sync::mpsc::{SyncSender, TrySendError, sync_channel},
    thread,
};

use tokio::sync::oneshot;

use tts_rs::{
    SynthesisEngine,
    engines::kokoro::{KokoroEngine, KokoroInferenceParams, KokoroModelParams},
};

use crate::{
    audio::OwnedAudio,
    config::{QwenSamplingConfig, TtsConfig, TtsProviderConfig},
    error::SophonError,
    model_registry::LoaderKind,
};
#[cfg(any(feature = "qwen-cpu", feature = "qwen-cuda", feature = "qwen-vulkan"))]
use qwentts_cpp::{
    Language as QwenLanguage, QwenTtsEngine, SamplingOptions as QwenSamplingOptions,
    Voice as QwenVoice, VoiceReference as QwenVoiceReference,
};

pub trait TtsProvider: Send {
    fn provider_id(&self) -> &'static str;
    fn model_id(&self) -> &str;
    fn capabilities(&self) -> TtsCapabilities;
    fn voices(&self) -> &[String];
    fn synthesize(&mut self, request: &TtsRequest) -> Result<OwnedAudio, SophonError>;
}

pub struct KokoroProvider {
    engine: KokoroEngine,
    model_id: String,
    voices: Vec<String>,
    default_voice: String,
}

impl KokoroProvider {
    pub fn load(
        model_path: &Path,
        model_id: impl Into<String>,
        default_voice: impl Into<String>,
        optimized_model_cache_path: Option<PathBuf>,
    ) -> Result<Self, SophonError> {
        let mut engine = KokoroEngine::new();
        engine
            .load_model_with_params(
                model_path,
                KokoroModelParams {
                    num_threads: None,
                    optimized_model_cache_path,
                },
            )
            .map_err(|error| SophonError::ModelUnavailable(error.to_string()))?;
        let voices = engine
            .list_voices()
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let default_voice = default_voice.into();
        if !voices.iter().any(|voice| voice == &default_voice) {
            return Err(SophonError::ModelUnavailable(format!(
                "configured default voice `{default_voice}` is not in the Kokoro model"
            )));
        }
        Ok(Self {
            engine,
            model_id: model_id.into(),
            voices,
            default_voice,
        })
    }

    fn validate_language(voice: &str, language: Option<&str>) -> Result<(), SophonError> {
        let Some(language) = language else {
            return Ok(());
        };
        let requested = language.to_ascii_lowercase();
        let prefix = &voice[..voice.len().min(2)];
        let expected = match prefix {
            "af" | "am" => "en-us",
            "bf" | "bm" => "en-gb",
            "ef" | "em" => "es",
            "ff" => "fr",
            "hf" | "hm" => "hi",
            "if" | "im" => "it",
            "jf" | "jm" => "ja",
            "pf" | "pm" => "pt-br",
            "zf" | "zm" => "cmn",
            _ => "en-us",
        };
        let compatible = requested == expected
            || (requested == "en" && expected.starts_with("en-"))
            || (expected == "pt-br" && requested == "pt")
            || (expected == "cmn" && matches!(requested.as_str(), "zh" | "zh-cn"));
        if !compatible {
            return Err(SophonError::InvalidTtsOptions(format!(
                "language `{language}` is incompatible with voice `{voice}`"
            )));
        }
        Ok(())
    }
}

#[cfg(any(feature = "qwen-cpu", feature = "qwen-cuda", feature = "qwen-vulkan"))]
fn validate_qwen_text_like(value: &str, field: &str, max_bytes: u64) -> Result<(), SophonError> {
    if value.len() as u64 > max_bytes {
        return Err(SophonError::ResourceLimit(format!(
            "{field} exceeds max_text_bytes ({max_bytes})"
        )));
    }
    if value.trim().is_empty() {
        return Err(SophonError::InvalidTtsOptions(format!(
            "{field} must not be empty"
        )));
    }
    if value.chars().any(|character| {
        character == '\0' || (character.is_control() && !character.is_whitespace())
    }) {
        return Err(SophonError::InvalidTtsOptions(format!(
            "{field} contains a disallowed control character"
        )));
    }
    Ok(())
}

#[cfg(any(feature = "qwen-cpu", feature = "qwen-cuda", feature = "qwen-vulkan"))]
pub fn normalize_qwen_language(language: Option<&str>) -> Result<QwenLanguage, SophonError> {
    let Some(language) = language else {
        return Ok(QwenLanguage::Auto);
    };
    let normalized = language.to_ascii_lowercase();
    match normalized.as_str() {
        "en" | "en-us" | "en-gb" | "en-au" | "en-ca" => Ok(QwenLanguage::English),
        "zh" | "zh-cn" | "zh-tw" | "cmn" => Ok(QwenLanguage::Chinese),
        "ja" | "ja-jp" => Ok(QwenLanguage::Japanese),
        "ko" | "ko-kr" => Ok(QwenLanguage::Korean),
        "de" | "de-de" => Ok(QwenLanguage::German),
        "fr" | "fr-fr" | "fr-ca" => Ok(QwenLanguage::French),
        "ru" | "ru-ru" => Ok(QwenLanguage::Russian),
        "pt" | "pt-br" | "pt-pt" => Ok(QwenLanguage::Portuguese),
        "es" | "es-es" | "es-mx" => Ok(QwenLanguage::Spanish),
        "it" | "it-it" => Ok(QwenLanguage::Italian),
        _ => Err(SophonError::InvalidTtsOptions(format!(
            "language `{language}` is unsupported by Qwen TTS"
        ))),
    }
}

#[cfg(any(feature = "qwen-cpu", feature = "qwen-cuda", feature = "qwen-vulkan"))]
pub struct QwenEngineAdapter {
    engine: QwenTtsEngine,
    sampling: QwenSamplingOptions,
}

#[cfg(any(feature = "qwen-cpu", feature = "qwen-cuda", feature = "qwen-vulkan"))]
impl QwenEngineAdapter {
    pub fn load(
        talker_path: &Path,
        codec_path: &Path,
        sampling: &QwenSamplingConfig,
        max_generated_audio_seconds: u64,
    ) -> Result<Self, SophonError> {
        let mut engine = QwenTtsEngine::new();
        engine
            .load_model(talker_path, codec_path)
            .map_err(|error| SophonError::ModelUnavailable(error.to_string()))?;
        let duration_tokens = engine
            .duration_sec_to_tokens(max_generated_audio_seconds as f32)
            .map_err(|error| SophonError::ModelUnavailable(error.to_string()))?;
        Ok(Self {
            engine,
            sampling: Self::effective_sampling(sampling, duration_tokens)?,
        })
    }

    fn effective_sampling(
        configured: &QwenSamplingConfig,
        duration_tokens: u32,
    ) -> Result<QwenSamplingOptions, SophonError> {
        let seed = configured
            .seed
            .map(i64::try_from)
            .transpose()
            .map_err(|_| SophonError::ModelUnavailable("Qwen seed exceeds i64::MAX".into()))?;
        Ok(QwenSamplingOptions {
            seed,
            max_new_tokens: configured.max_new_tokens.min(duration_tokens),
            temperature: configured.temperature,
            top_k: configured.top_k,
            top_p: configured.top_p,
            repetition_penalty: configured.repetition_penalty,
        })
    }

    pub fn speakers(&self) -> Result<Vec<String>, SophonError> {
        self.engine
            .speakers()
            .map_err(|error| SophonError::ModelUnavailable(error.to_string()))
    }

    pub fn extract_voice_reference(
        &mut self,
        samples_24khz_mono: &[f32],
    ) -> Result<QwenVoiceReference, SophonError> {
        self.engine
            .extract_voice_reference(samples_24khz_mono)
            .map_err(|error| SophonError::SynthesisFailed(error.to_string()))
    }

    pub fn synthesize(
        &mut self,
        text: &str,
        language: QwenLanguage,
        voice: QwenVoice<'_>,
    ) -> Result<OwnedAudio, SophonError> {
        let result = self
            .engine
            .synthesize(
                text,
                Some(qwentts_cpp::SynthesisOptions {
                    language,
                    sampling: self.sampling.clone(),
                    voice,
                }),
            )
            .map_err(|error| SophonError::SynthesisFailed(error.to_string()))?;
        if result.sample_rate != 24_000 {
            return Err(SophonError::SynthesisFailed(format!(
                "qwentts.cpp returned unexpected sample rate {}",
                result.sample_rate
            )));
        }
        Ok(OwnedAudio {
            samples: result.samples,
            sample_rate: result.sample_rate,
        })
    }
}

#[cfg(any(feature = "qwen-cpu", feature = "qwen-cuda", feature = "qwen-vulkan"))]
trait QwenProviderEngine: Send {
    fn speakers(&self) -> Result<Vec<String>, SophonError>;
    fn synthesize_default(
        &mut self,
        text: &str,
        language: QwenLanguage,
    ) -> Result<OwnedAudio, SophonError>;
    fn synthesize_named(
        &mut self,
        text: &str,
        language: QwenLanguage,
        speaker: &str,
    ) -> Result<OwnedAudio, SophonError>;
    fn synthesize_design(
        &mut self,
        text: &str,
        language: QwenLanguage,
        description: &str,
    ) -> Result<OwnedAudio, SophonError>;
    fn synthesize_clone(
        &mut self,
        text: &str,
        language: QwenLanguage,
        samples_24khz_mono: &[f32],
        transcript: Option<&str>,
    ) -> Result<OwnedAudio, SophonError>;
}

#[cfg(any(feature = "qwen-cpu", feature = "qwen-cuda", feature = "qwen-vulkan"))]
impl QwenProviderEngine for QwenEngineAdapter {
    fn speakers(&self) -> Result<Vec<String>, SophonError> {
        self.speakers()
    }

    fn synthesize_default(
        &mut self,
        text: &str,
        language: QwenLanguage,
    ) -> Result<OwnedAudio, SophonError> {
        self.synthesize(text, language, QwenVoice::Default)
    }

    fn synthesize_named(
        &mut self,
        text: &str,
        language: QwenLanguage,
        speaker: &str,
    ) -> Result<OwnedAudio, SophonError> {
        self.synthesize(text, language, QwenVoice::Named(speaker))
    }

    fn synthesize_design(
        &mut self,
        text: &str,
        language: QwenLanguage,
        description: &str,
    ) -> Result<OwnedAudio, SophonError> {
        self.synthesize(text, language, QwenVoice::Design(description))
    }

    fn synthesize_clone(
        &mut self,
        text: &str,
        language: QwenLanguage,
        samples_24khz_mono: &[f32],
        transcript: Option<&str>,
    ) -> Result<OwnedAudio, SophonError> {
        let reference = self.extract_voice_reference(samples_24khz_mono)?;
        let voice = match transcript {
            Some(transcript) => QwenVoice::CloneWithTranscript {
                reference: &reference,
                transcript,
            },
            None => QwenVoice::Clone(&reference),
        };
        self.synthesize(text, language, voice)
    }
}

#[cfg(any(feature = "qwen-cpu", feature = "qwen-cuda", feature = "qwen-vulkan"))]
pub struct QwenTtsBaseProvider {
    engine: Box<dyn QwenProviderEngine>,
    model_id: String,
    max_text_bytes: u64,
}

#[cfg(any(feature = "qwen-cpu", feature = "qwen-cuda", feature = "qwen-vulkan"))]
impl QwenTtsBaseProvider {
    pub fn new(
        engine: QwenEngineAdapter,
        model_id: impl Into<String>,
        max_text_bytes: u64,
    ) -> Self {
        Self::with_engine(Box::new(engine), model_id, max_text_bytes)
    }

    fn with_engine(
        engine: Box<dyn QwenProviderEngine>,
        model_id: impl Into<String>,
        max_text_bytes: u64,
    ) -> Self {
        Self {
            engine,
            model_id: model_id.into(),
            max_text_bytes,
        }
    }
}

#[cfg(any(feature = "qwen-cpu", feature = "qwen-cuda", feature = "qwen-vulkan"))]
pub struct QwenTtsCustomVoiceProvider {
    engine: Box<dyn QwenProviderEngine>,
    model_id: String,
    voices: Vec<String>,
    default_voice: String,
    max_text_bytes: u64,
}

#[cfg(any(feature = "qwen-cpu", feature = "qwen-cuda", feature = "qwen-vulkan"))]
impl QwenTtsCustomVoiceProvider {
    pub fn new(
        engine: QwenEngineAdapter,
        model_id: impl Into<String>,
        default_voice: impl Into<String>,
        max_text_bytes: u64,
    ) -> Result<Self, SophonError> {
        Self::with_engine(Box::new(engine), model_id, default_voice, max_text_bytes)
    }

    fn with_engine(
        engine: Box<dyn QwenProviderEngine>,
        model_id: impl Into<String>,
        default_voice: impl Into<String>,
        max_text_bytes: u64,
    ) -> Result<Self, SophonError> {
        let voices = engine.speakers()?;
        let default_voice = default_voice.into();
        if !voices.iter().any(|voice| voice == &default_voice) {
            return Err(SophonError::ModelUnavailable(format!(
                "configured default voice `{default_voice}` is not in the Qwen CustomVoice model"
            )));
        }
        Ok(Self {
            engine,
            model_id: model_id.into(),
            voices,
            default_voice,
            max_text_bytes,
        })
    }
}

#[cfg(any(feature = "qwen-cpu", feature = "qwen-cuda", feature = "qwen-vulkan"))]
impl TtsProvider for QwenTtsCustomVoiceProvider {
    fn provider_id(&self) -> &'static str {
        "qwentts-cpp"
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn capabilities(&self) -> TtsCapabilities {
        TtsCapabilities {
            named_voices: true,
            voice_cloning: false,
            voice_design: false,
            speed_control: false,
        }
    }

    fn voices(&self) -> &[String] {
        &self.voices
    }

    fn synthesize(&mut self, request: &TtsRequest) -> Result<OwnedAudio, SophonError> {
        validate_capabilities(self, request)?;
        validate_qwen_text_like(&request.text, "synthesis text", self.max_text_bytes)?;
        if request.speed != 1.0 {
            return Err(SophonError::InvalidTtsOptions(
                "Qwen TTS supports only unit speed".into(),
            ));
        }
        let language = normalize_qwen_language(request.language.as_deref())?;
        let speaker = match &request.voice {
            VoiceIntent::Default => &self.default_voice,
            VoiceIntent::Named(speaker) => speaker,
            VoiceIntent::Clone { .. } | VoiceIntent::Design(_) => unreachable!("validated above"),
        };
        self.engine
            .synthesize_named(&request.text, language, speaker)
    }
}

#[cfg(any(feature = "qwen-cpu", feature = "qwen-cuda", feature = "qwen-vulkan"))]
pub struct QwenTtsVoiceDesignProvider {
    engine: Box<dyn QwenProviderEngine>,
    model_id: String,
    default_voice_description: String,
    max_text_bytes: u64,
}

#[cfg(any(feature = "qwen-cpu", feature = "qwen-cuda", feature = "qwen-vulkan"))]
impl QwenTtsVoiceDesignProvider {
    pub fn new(
        engine: QwenEngineAdapter,
        model_id: impl Into<String>,
        default_voice_description: impl Into<String>,
        max_text_bytes: u64,
    ) -> Self {
        Self::with_engine(
            Box::new(engine),
            model_id,
            default_voice_description,
            max_text_bytes,
        )
    }

    fn with_engine(
        engine: Box<dyn QwenProviderEngine>,
        model_id: impl Into<String>,
        default_voice_description: impl Into<String>,
        max_text_bytes: u64,
    ) -> Self {
        Self {
            engine,
            model_id: model_id.into(),
            default_voice_description: default_voice_description.into(),
            max_text_bytes,
        }
    }
}

#[cfg(any(feature = "qwen-cpu", feature = "qwen-cuda", feature = "qwen-vulkan"))]
impl TtsProvider for QwenTtsVoiceDesignProvider {
    fn provider_id(&self) -> &'static str {
        "qwentts-cpp"
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn capabilities(&self) -> TtsCapabilities {
        TtsCapabilities {
            named_voices: false,
            voice_cloning: false,
            voice_design: true,
            speed_control: false,
        }
    }

    fn voices(&self) -> &[String] {
        &[]
    }

    fn synthesize(&mut self, request: &TtsRequest) -> Result<OwnedAudio, SophonError> {
        validate_capabilities(self, request)?;
        if request.speed != 1.0 {
            return Err(SophonError::InvalidTtsOptions(
                "Qwen TTS supports only unit speed".into(),
            ));
        }
        validate_qwen_text_like(&request.text, "synthesis text", self.max_text_bytes)?;
        let language = normalize_qwen_language(request.language.as_deref())?;
        let description = match &request.voice {
            VoiceIntent::Default => &self.default_voice_description,
            VoiceIntent::Design(description) => description,
            VoiceIntent::Named(_) | VoiceIntent::Clone { .. } => unreachable!("validated above"),
        };
        validate_qwen_text_like(description, "voice description", self.max_text_bytes)?;
        self.engine
            .synthesize_design(&request.text, language, description)
    }
}

#[cfg(any(feature = "qwen-cpu", feature = "qwen-cuda", feature = "qwen-vulkan"))]
impl TtsProvider for QwenTtsBaseProvider {
    fn provider_id(&self) -> &'static str {
        "qwentts-cpp"
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn capabilities(&self) -> TtsCapabilities {
        TtsCapabilities {
            named_voices: false,
            voice_cloning: true,
            voice_design: false,
            speed_control: false,
        }
    }

    fn voices(&self) -> &[String] {
        &[]
    }

    fn synthesize(&mut self, request: &TtsRequest) -> Result<OwnedAudio, SophonError> {
        validate_capabilities(self, request)?;
        if request.speed != 1.0 {
            return Err(SophonError::InvalidTtsOptions(
                "Qwen TTS supports only unit speed".into(),
            ));
        }
        validate_qwen_text_like(&request.text, "synthesis text", self.max_text_bytes)?;
        let language = normalize_qwen_language(request.language.as_deref())?;
        match &request.voice {
            VoiceIntent::Default => self.engine.synthesize_default(&request.text, language),
            VoiceIntent::Clone {
                reference,
                transcript,
            } => {
                if let Some(transcript) = transcript {
                    validate_qwen_text_like(transcript, "clone transcript", self.max_text_bytes)?;
                }
                if reference.sample_rate != 24_000 {
                    return Err(SophonError::InvalidTtsOptions(
                        "Qwen clone references must be 24 kHz mono PCM".into(),
                    ));
                }
                self.engine.synthesize_clone(
                    &request.text,
                    language,
                    &reference.samples,
                    transcript.as_deref(),
                )
            }
            VoiceIntent::Named(_) | VoiceIntent::Design(_) => unreachable!("validated above"),
        }
    }
}

pub enum TtsProviderModel {
    KokoroDirectory(PathBuf),
    Qwen {
        model_id: String,
        kind: LoaderKind,
        talker_path: PathBuf,
        codec_path: PathBuf,
    },
}

pub fn create_tts_provider(
    config: &TtsConfig,
    model: TtsProviderModel,
    optimized_model_cache_path: Option<PathBuf>,
) -> Result<Box<dyn TtsProvider>, SophonError> {
    match (&config.provider, model) {
        (
            TtsProviderConfig::Kokoro {
                model_id,
                default_voice,
            },
            TtsProviderModel::KokoroDirectory(model_dir),
        ) => Ok(Box::new(KokoroProvider::load(
            &model_dir,
            model_id,
            default_voice,
            optimized_model_cache_path,
        )?)),
        (
            variant,
            TtsProviderModel::Qwen {
                model_id: resolved_id,
                kind,
                talker_path,
                codec_path,
            },
        ) => {
            let (model_id, expected_kind, sampling) = match variant {
                TtsProviderConfig::QwenBase {
                    model_id, sampling, ..
                } => (model_id, LoaderKind::Base, sampling),
                TtsProviderConfig::QwenCustomVoice {
                    model_id, sampling, ..
                } => (model_id, LoaderKind::CustomVoice, sampling),
                TtsProviderConfig::QwenVoiceDesign {
                    model_id, sampling, ..
                } => (model_id, LoaderKind::VoiceDesign, sampling),
                TtsProviderConfig::Kokoro { .. } => {
                    return Err(SophonError::ModelUnavailable(
                        "Kokoro configuration cannot load Qwen artifacts".into(),
                    ));
                }
            };
            if resolved_id != *model_id || kind != expected_kind {
                return Err(SophonError::ModelUnavailable(format!(
                    "typed TTS configuration does not match resolved model `{resolved_id}`"
                )));
            }
            #[cfg(any(feature = "qwen-cpu", feature = "qwen-cuda", feature = "qwen-vulkan"))]
            {
                let engine = QwenEngineAdapter::load(
                    &talker_path,
                    &codec_path,
                    sampling,
                    config.operational.max_generated_audio_seconds,
                )?;
                let provider: Box<dyn TtsProvider> = match variant {
                    TtsProviderConfig::QwenBase { .. } => Box::new(QwenTtsBaseProvider::new(
                        engine,
                        model_id,
                        config.operational.max_text_bytes,
                    )),
                    TtsProviderConfig::QwenCustomVoice { default_voice, .. } => {
                        Box::new(QwenTtsCustomVoiceProvider::new(
                            engine,
                            model_id,
                            default_voice,
                            config.operational.max_text_bytes,
                        )?)
                    }
                    TtsProviderConfig::QwenVoiceDesign {
                        default_voice_description,
                        ..
                    } => Box::new(QwenTtsVoiceDesignProvider::new(
                        engine,
                        model_id,
                        default_voice_description,
                        config.operational.max_text_bytes,
                    )),
                    TtsProviderConfig::Kokoro { .. } => unreachable!(),
                };
                Ok(provider)
            }
            #[cfg(not(any(feature = "qwen-cpu", feature = "qwen-cuda", feature = "qwen-vulkan")))]
            {
                let _ = (sampling, talker_path, codec_path);
                Err(SophonError::ModelUnavailable(
                    "this Sophon build has no Qwen backend".into(),
                ))
            }
        }
        (TtsProviderConfig::QwenBase { .. }, TtsProviderModel::KokoroDirectory(_))
        | (TtsProviderConfig::QwenCustomVoice { .. }, TtsProviderModel::KokoroDirectory(_))
        | (TtsProviderConfig::QwenVoiceDesign { .. }, TtsProviderModel::KokoroDirectory(_)) => {
            Err(SophonError::ModelUnavailable(
                "Qwen configuration requires resolved talker and codec artifacts".into(),
            ))
        }
    }
}

impl TtsProvider for KokoroProvider {
    fn provider_id(&self) -> &'static str {
        "tts-rs"
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn capabilities(&self) -> TtsCapabilities {
        TtsCapabilities {
            named_voices: true,
            voice_cloning: false,
            voice_design: false,
            speed_control: true,
        }
    }

    fn voices(&self) -> &[String] {
        &self.voices
    }

    fn synthesize(&mut self, request: &TtsRequest) -> Result<OwnedAudio, SophonError> {
        validate_capabilities(self, request)?;
        if !request.speed.is_finite() || !(0.5..=2.0).contains(&request.speed) {
            return Err(SophonError::InvalidTtsOptions(
                "speed must be finite and between 0.5 and 2.0".into(),
            ));
        }
        let voice = match &request.voice {
            VoiceIntent::Default => &self.default_voice,
            VoiceIntent::Named(voice) => voice,
            VoiceIntent::Clone { .. } | VoiceIntent::Design(_) => {
                return Err(SophonError::UnsupportedCapability(
                    "Kokoro supports only default and named voices".into(),
                ));
            }
        };
        Self::validate_language(voice, request.language.as_deref())?;
        let result = self
            .engine
            .synthesize(
                &request.text,
                Some(KokoroInferenceParams {
                    voice: voice.clone(),
                    speed: request.speed as f32,
                    style_index: None,
                }),
            )
            .map_err(|error| SophonError::SynthesisFailed(error.to_string()))?;
        if result.sample_rate != 24_000 {
            return Err(SophonError::SynthesisFailed(format!(
                "Kokoro returned unexpected sample rate {}",
                result.sample_rate
            )));
        }
        Ok(OwnedAudio {
            samples: result.samples,
            sample_rate: result.sample_rate,
        })
    }
}

struct TtsWorkItem {
    request: TtsRequest,
    response: oneshot::Sender<Result<OwnedAudio, SophonError>>,
}

#[derive(Clone)]
pub struct TtsWorker {
    sender: SyncSender<TtsWorkItem>,
    provider_id: &'static str,
    model_id: String,
    capabilities: TtsCapabilities,
    voices: Vec<String>,
}

impl TtsWorker {
    pub fn new(
        mut provider: Box<dyn TtsProvider>,
        capacity: usize,
        max_generated_audio_seconds: u64,
    ) -> Self {
        let provider_id = provider.provider_id();
        let model_id = provider.model_id().to_owned();
        let capabilities = provider.capabilities();
        let voices = provider.voices().to_vec();
        let (sender, receiver) = sync_channel::<TtsWorkItem>(capacity);
        thread::Builder::new()
            .name("sophon-tts-worker".into())
            .spawn(move || {
                while let Ok(work) = receiver.recv() {
                    let result = provider.synthesize(&work.request).and_then(|audio| {
                        if audio.sample_rate == 0 {
                            return Err(SophonError::SynthesisFailed(
                                "provider returned a zero sample rate".into(),
                            ));
                        }
                        let maximum_frames =
                            u128::from(audio.sample_rate) * u128::from(max_generated_audio_seconds);
                        if audio.samples.len() as u128 > maximum_frames {
                            return Err(SophonError::ResourceLimit(format!(
                                "generated audio exceeds {max_generated_audio_seconds} seconds"
                            )));
                        }
                        Ok(audio)
                    });
                    let _ = work.response.send(result);
                }
            })
            .expect("failed to create TTS worker");
        Self {
            sender,
            provider_id,
            model_id,
            capabilities,
            voices,
        }
    }

    pub fn provider_id(&self) -> &'static str {
        self.provider_id
    }

    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    pub fn capabilities(&self) -> TtsCapabilities {
        self.capabilities
    }

    pub fn voices(&self) -> &[String] {
        &self.voices
    }

    pub async fn synthesize(&self, request: TtsRequest) -> Result<OwnedAudio, SophonError> {
        let (response, receiver) = oneshot::channel();
        match self.sender.try_send(TtsWorkItem { request, response }) {
            Ok(()) => receiver
                .await
                .map_err(|_| SophonError::SynthesisFailed("TTS worker stopped".into()))?,
            Err(TrySendError::Full(_)) => Err(SophonError::ResourceLimit(
                "TTS inference queue is full".into(),
            )),
            Err(TrySendError::Disconnected(_)) => Err(SophonError::ModelUnavailable(
                "TTS worker is unavailable".into(),
            )),
        }
    }
}

pub fn validate_capabilities(
    provider: &dyn TtsProvider,
    request: &TtsRequest,
) -> Result<(), SophonError> {
    let capabilities = provider.capabilities();
    match &request.voice {
        VoiceIntent::Default => Ok(()),
        VoiceIntent::Named(voice) => {
            if !capabilities.named_voices {
                return Err(SophonError::UnsupportedCapability(
                    "named voices are not supported".into(),
                ));
            }
            if !provider.voices().iter().any(|available| available == voice) {
                return Err(SophonError::InvalidTtsOptions(format!(
                    "voice `{voice}` is not available"
                )));
            }
            Ok(())
        }
        VoiceIntent::Clone { .. } if !capabilities.voice_cloning => Err(
            SophonError::UnsupportedCapability("one-shot voice cloning is not supported".into()),
        ),
        VoiceIntent::Design(_) if !capabilities.voice_design => Err(
            SophonError::UnsupportedCapability("voice design is not supported".into()),
        ),
        VoiceIntent::Clone { .. } | VoiceIntent::Design(_) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::{Arc, Mutex},
        time::Duration,
    };

    #[cfg(any(feature = "qwen-cpu", feature = "qwen-cuda", feature = "qwen-vulkan"))]
    #[derive(Debug, PartialEq)]
    enum QwenCall {
        Default(QwenLanguage),
        Named(QwenLanguage, String),
        Design(QwenLanguage, String),
        Clone(QwenLanguage, Vec<f32>, Option<String>),
    }

    #[cfg(any(feature = "qwen-cpu", feature = "qwen-cuda", feature = "qwen-vulkan"))]
    struct FixtureQwenEngine {
        calls: Arc<Mutex<Vec<QwenCall>>>,
        speakers: Vec<String>,
        fail_once: bool,
    }

    #[cfg(any(feature = "qwen-cpu", feature = "qwen-cuda", feature = "qwen-vulkan"))]
    impl FixtureQwenEngine {
        fn respond(&mut self, call: QwenCall) -> Result<OwnedAudio, SophonError> {
            self.calls.lock().unwrap().push(call);
            if self.fail_once {
                self.fail_once = false;
                return Err(SophonError::SynthesisFailed("fixture failure".into()));
            }
            Ok(OwnedAudio {
                samples: vec![0.25],
                sample_rate: 24_000,
            })
        }
    }

    #[cfg(any(feature = "qwen-cpu", feature = "qwen-cuda", feature = "qwen-vulkan"))]
    impl QwenProviderEngine for FixtureQwenEngine {
        fn speakers(&self) -> Result<Vec<String>, SophonError> {
            Ok(self.speakers.clone())
        }

        fn synthesize_default(
            &mut self,
            _: &str,
            language: QwenLanguage,
        ) -> Result<OwnedAudio, SophonError> {
            self.respond(QwenCall::Default(language))
        }

        fn synthesize_named(
            &mut self,
            _: &str,
            language: QwenLanguage,
            speaker: &str,
        ) -> Result<OwnedAudio, SophonError> {
            self.respond(QwenCall::Named(language, speaker.into()))
        }

        fn synthesize_design(
            &mut self,
            _: &str,
            language: QwenLanguage,
            description: &str,
        ) -> Result<OwnedAudio, SophonError> {
            self.respond(QwenCall::Design(language, description.into()))
        }

        fn synthesize_clone(
            &mut self,
            _: &str,
            language: QwenLanguage,
            samples: &[f32],
            transcript: Option<&str>,
        ) -> Result<OwnedAudio, SophonError> {
            self.respond(QwenCall::Clone(
                language,
                samples.to_vec(),
                transcript.map(str::to_owned),
            ))
        }
    }

    struct FixtureProvider {
        calls: Arc<Mutex<Vec<TtsRequest>>>,
        fail_first: bool,
        capabilities: TtsCapabilities,
        voices: Vec<String>,
    }

    impl TtsProvider for FixtureProvider {
        fn provider_id(&self) -> &'static str {
            "fixture"
        }

        fn model_id(&self) -> &str {
            "fixture-model"
        }

        fn capabilities(&self) -> TtsCapabilities {
            self.capabilities
        }

        fn voices(&self) -> &[String] {
            &self.voices
        }

        fn synthesize(&mut self, request: &TtsRequest) -> Result<OwnedAudio, SophonError> {
            validate_capabilities(self, request)?;
            self.calls.lock().unwrap().push(request.clone());
            if self.fail_first {
                self.fail_first = false;
                return Err(SophonError::SynthesisFailed("fixture failure".into()));
            }
            std::thread::sleep(Duration::from_millis(20));
            let frames = if request.text == "oversize" { 11 } else { 1 };
            Ok(OwnedAudio {
                samples: vec![request.speed as f32; frames],
                sample_rate: 10,
            })
        }
    }

    fn capabilities() -> TtsCapabilities {
        TtsCapabilities {
            named_voices: true,
            voice_cloning: false,
            voice_design: false,
            speed_control: true,
        }
    }

    fn request(text: &str, voice: VoiceIntent) -> TtsRequest {
        TtsRequest {
            text: text.into(),
            language: Some("en".into()),
            speed: 1.25,
            voice,
        }
    }

    fn fixture(calls: Arc<Mutex<Vec<TtsRequest>>>, fail_first: bool) -> Box<dyn TtsProvider> {
        Box::new(FixtureProvider {
            calls,
            fail_first,
            capabilities: capabilities(),
            voices: vec!["af_heart".into(), "am_adam".into()],
        })
    }

    #[cfg(any(feature = "qwen-cpu", feature = "qwen-cuda", feature = "qwen-vulkan"))]
    #[test]
    fn qwen_language_normalization_is_conservative_and_case_insensitive() {
        assert_eq!(normalize_qwen_language(None).unwrap(), QwenLanguage::Auto);
        for (tag, expected) in [
            ("EN-us", QwenLanguage::English),
            ("zh-CN", QwenLanguage::Chinese),
            ("ja-JP", QwenLanguage::Japanese),
            ("ko-KR", QwenLanguage::Korean),
            ("de-DE", QwenLanguage::German),
            ("fr-CA", QwenLanguage::French),
            ("ru-RU", QwenLanguage::Russian),
            ("pt-BR", QwenLanguage::Portuguese),
            ("es-MX", QwenLanguage::Spanish),
            ("it-IT", QwenLanguage::Italian),
        ] {
            assert_eq!(normalize_qwen_language(Some(tag)).unwrap(), expected);
        }
        for unsupported in ["", "en-US-extra", "ar", "en_US", " english "] {
            assert!(matches!(
                normalize_qwen_language(Some(unsupported)),
                Err(SophonError::InvalidTtsOptions(_))
            ));
        }
    }

    #[cfg(any(feature = "qwen-cpu", feature = "qwen-cuda", feature = "qwen-vulkan"))]
    #[test]
    fn qwen_base_supports_default_and_temporary_one_shot_clone_references() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let mut provider = QwenTtsBaseProvider::with_engine(
            Box::new(FixtureQwenEngine {
                calls: calls.clone(),
                speakers: Vec::new(),
                fail_once: false,
            }),
            "qwen-base-fixture",
            1024,
        );
        assert_eq!(provider.provider_id(), "qwentts-cpp");
        assert!(provider.capabilities().voice_cloning);
        assert!(!provider.capabilities().named_voices);
        assert!(
            provider
                .synthesize(&TtsRequest {
                    text: "hello".into(),
                    language: None,
                    speed: 1.0,
                    voice: VoiceIntent::Default,
                })
                .is_ok()
        );
        assert!(
            provider
                .synthesize(&TtsRequest {
                    text: "clone".into(),
                    language: Some("EN-us".into()),
                    speed: 1.0,
                    voice: VoiceIntent::Clone {
                        reference: OwnedAudio {
                            samples: vec![0.1, -0.1],
                            sample_rate: 24_000,
                        },
                        transcript: Some("reference words".into()),
                    },
                })
                .is_ok()
        );
        assert_eq!(
            *calls.lock().unwrap(),
            vec![
                QwenCall::Default(QwenLanguage::Auto),
                QwenCall::Clone(
                    QwenLanguage::English,
                    vec![0.1, -0.1],
                    Some("reference words".into())
                ),
            ]
        );
        assert!(matches!(
            provider.synthesize(&TtsRequest {
                text: "bad speed".into(),
                language: None,
                speed: 1.1,
                voice: VoiceIntent::Default,
            }),
            Err(SophonError::InvalidTtsOptions(_))
        ));
    }

    #[cfg(any(feature = "qwen-cpu", feature = "qwen-cuda", feature = "qwen-vulkan"))]
    #[test]
    fn qwen_custom_voice_enumerates_validates_and_synthesizes_named_speakers() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let engine = || FixtureQwenEngine {
            calls: calls.clone(),
            speakers: vec!["vivian".into(), "ryan".into()],
            fail_once: false,
        };
        assert!(matches!(
            QwenTtsCustomVoiceProvider::with_engine(
                Box::new(engine()),
                "qwen-custom-fixture",
                "missing",
                1024,
            ),
            Err(SophonError::ModelUnavailable(_))
        ));
        let mut provider = QwenTtsCustomVoiceProvider::with_engine(
            Box::new(engine()),
            "qwen-custom-fixture",
            "vivian",
            1024,
        )
        .unwrap();
        assert_eq!(provider.voices(), ["vivian", "ryan"]);
        assert!(provider.capabilities().named_voices);
        for voice in [VoiceIntent::Default, VoiceIntent::Named("ryan".into())] {
            provider
                .synthesize(&TtsRequest {
                    text: "hello".into(),
                    language: Some("zh-CN".into()),
                    speed: 1.0,
                    voice,
                })
                .unwrap();
        }
        assert_eq!(
            *calls.lock().unwrap(),
            vec![
                QwenCall::Named(QwenLanguage::Chinese, "vivian".into()),
                QwenCall::Named(QwenLanguage::Chinese, "ryan".into()),
            ]
        );
        assert!(matches!(
            provider.synthesize(&TtsRequest {
                text: "hello".into(),
                language: None,
                speed: 1.0,
                voice: VoiceIntent::Named("missing".into()),
            }),
            Err(SophonError::InvalidTtsOptions(_))
        ));
    }

    #[cfg(any(feature = "qwen-cpu", feature = "qwen-cuda", feature = "qwen-vulkan"))]
    #[test]
    fn qwen_voice_design_uses_configured_default_and_request_override() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let mut provider = QwenTtsVoiceDesignProvider::with_engine(
            Box::new(FixtureQwenEngine {
                calls: calls.clone(),
                speakers: Vec::new(),
                fail_once: false,
            }),
            "qwen-design-fixture",
            "warm default",
            1024,
        );
        assert!(provider.capabilities().voice_design);
        for voice in [
            VoiceIntent::Default,
            VoiceIntent::Design("bright override".into()),
        ] {
            provider
                .synthesize(&TtsRequest {
                    text: "hello".into(),
                    language: Some("it-IT".into()),
                    speed: 1.0,
                    voice,
                })
                .unwrap();
        }
        assert_eq!(
            *calls.lock().unwrap(),
            vec![
                QwenCall::Design(QwenLanguage::Italian, "warm default".into()),
                QwenCall::Design(QwenLanguage::Italian, "bright override".into()),
            ]
        );
    }

    #[cfg(any(feature = "qwen-cpu", feature = "qwen-cuda", feature = "qwen-vulkan"))]
    #[test]
    fn qwen_providers_apply_independent_limits_and_recover_after_native_failure() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let mut base = QwenTtsBaseProvider::with_engine(
            Box::new(FixtureQwenEngine {
                calls: calls.clone(),
                speakers: Vec::new(),
                fail_once: true,
            }),
            "qwen-base-fixture",
            4,
        );
        let default_request = |text: &str| TtsRequest {
            text: text.into(),
            language: None,
            speed: 1.0,
            voice: VoiceIntent::Default,
        };
        assert!(matches!(
            base.synthesize(&default_request("oversize")),
            Err(SophonError::ResourceLimit(_))
        ));
        assert!(matches!(
            base.synthesize(&TtsRequest {
                text: "okay".into(),
                language: None,
                speed: 1.0,
                voice: VoiceIntent::Clone {
                    reference: OwnedAudio {
                        samples: vec![0.0],
                        sample_rate: 24_000,
                    },
                    transcript: Some("large".into()),
                },
            }),
            Err(SophonError::ResourceLimit(_))
        ));
        assert!(matches!(
            base.synthesize(&default_request("fail")),
            Err(SophonError::SynthesisFailed(_))
        ));
        assert!(base.synthesize(&default_request("okay")).is_ok());
        assert_eq!(
            *calls.lock().unwrap(),
            vec![
                QwenCall::Default(QwenLanguage::Auto),
                QwenCall::Default(QwenLanguage::Auto)
            ]
        );

        let design_calls = Arc::new(Mutex::new(Vec::new()));
        let mut design = QwenTtsVoiceDesignProvider::with_engine(
            Box::new(FixtureQwenEngine {
                calls: design_calls.clone(),
                speakers: Vec::new(),
                fail_once: false,
            }),
            "qwen-design-fixture",
            "voice",
            5,
        );
        assert!(design.synthesize(&default_request("hello")).is_ok());
        assert!(matches!(
            design.synthesize(&TtsRequest {
                voice: VoiceIntent::Design("voices".into()),
                ..default_request("hello")
            }),
            Err(SophonError::ResourceLimit(_))
        ));
    }

    #[test]
    fn typed_provider_factory_rejects_mismatched_configuration_and_artifacts() {
        let root = tempfile::tempdir().unwrap();
        let paths = crate::config::ConfigPaths::from_homes(
            root.path().join("config"),
            root.path().join("cache"),
        );
        let mut config = crate::config::Config::load(&paths).unwrap().tts.unwrap();
        let model_id = "qwen3-tts-0.6b-base-q8_0";
        let resolved = TtsProviderModel::Qwen {
            model_id: model_id.into(),
            kind: LoaderKind::Base,
            talker_path: root.path().join("talker.gguf"),
            codec_path: root.path().join("codec.gguf"),
        };
        assert!(matches!(
            create_tts_provider(&config, resolved, None),
            Err(SophonError::ModelUnavailable(_))
        ));

        config.provider = TtsProviderConfig::QwenBase {
            model_id: model_id.into(),
            default_clone_reference: None,
            default_clone_transcript: None,
            sampling: QwenSamplingConfig::default(),
        };
        assert!(matches!(
            create_tts_provider(
                &config,
                TtsProviderModel::KokoroDirectory(root.path().into()),
                None,
            ),
            Err(SophonError::ModelUnavailable(_))
        ));
    }

    #[cfg(any(feature = "qwen-cpu", feature = "qwen-cuda", feature = "qwen-vulkan"))]
    #[test]
    fn qwen_adapter_applies_sampling_and_native_duration_token_caps() {
        let configured = QwenSamplingConfig {
            seed: Some(42),
            max_new_tokens: 2048,
            temperature: 0.7,
            top_k: 25,
            top_p: 0.8,
            repetition_penalty: 1.1,
        };
        let effective = QwenEngineAdapter::effective_sampling(&configured, 750).unwrap();
        assert_eq!(effective.seed, Some(42));
        assert_eq!(effective.max_new_tokens, 750);
        assert_eq!(effective.temperature, 0.7);
        assert_eq!(effective.top_k, 25);
        assert_eq!(effective.top_p, 0.8);
        assert_eq!(effective.repetition_penalty, 1.1);
    }

    #[test]
    fn capability_validation_rejects_unknown_named_clone_and_design_intents() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let provider = fixture(calls, false);
        assert!(validate_capabilities(&*provider, &request("ok", VoiceIntent::Default)).is_ok());
        assert!(
            validate_capabilities(
                &*provider,
                &request("ok", VoiceIntent::Named("af_heart".into()))
            )
            .is_ok()
        );
        assert!(matches!(
            validate_capabilities(
                &*provider,
                &request("ok", VoiceIntent::Named("missing".into()))
            ),
            Err(SophonError::InvalidTtsOptions(_))
        ));
        assert!(matches!(
            validate_capabilities(
                &*provider,
                &request(
                    "ok",
                    VoiceIntent::Clone {
                        reference: OwnedAudio {
                            samples: vec![0.0],
                            sample_rate: 24_000,
                        },
                        transcript: None,
                    }
                )
            ),
            Err(SophonError::UnsupportedCapability(_))
        ));
        assert!(matches!(
            validate_capabilities(
                &*provider,
                &request("ok", VoiceIntent::Design("warm voice".into()))
            ),
            Err(SophonError::UnsupportedCapability(_))
        ));
    }

    #[test]
    fn kokoro_validates_language_voice_compatibility() {
        assert!(KokoroProvider::validate_language("af_heart", Some("en")).is_ok());
        assert!(KokoroProvider::validate_language("bf_emma", Some("en-gb")).is_ok());
        assert!(KokoroProvider::validate_language("zf_xiaobei", Some("zh-cn")).is_ok());
        assert!(matches!(
            KokoroProvider::validate_language("ff_siwis", Some("de")),
            Err(SophonError::InvalidTtsOptions(_))
        ));
    }

    #[tokio::test]
    async fn worker_preserves_request_options_fifo_and_recovers_after_failure() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let worker = TtsWorker::new(fixture(calls.clone(), true), 2, 10);
        assert_eq!(worker.provider_id(), "fixture");
        assert_eq!(worker.model_id(), "fixture-model");
        assert_eq!(worker.voices(), ["af_heart", "am_adam"]);
        assert!(matches!(
            worker
                .synthesize(request("first", VoiceIntent::Default))
                .await,
            Err(SophonError::SynthesisFailed(_))
        ));
        let second_request = request("second", VoiceIntent::Named("am_adam".into()));
        let third_request = request("third", VoiceIntent::Default);
        let (second, third) = tokio::join!(
            worker.synthesize(second_request.clone()),
            worker.synthesize(third_request.clone())
        );
        assert_eq!(second.unwrap().samples, vec![1.25]);
        assert!(third.is_ok());
        assert_eq!(
            *calls.lock().unwrap(),
            vec![
                request("first", VoiceIntent::Default),
                second_request,
                third_request
            ]
        );
    }

    #[tokio::test]
    async fn worker_rejects_full_queue_and_oversized_output() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let worker = TtsWorker::new(fixture(calls, false), 1, 1);
        let first = worker.synthesize(request("first", VoiceIntent::Default));
        tokio::pin!(first);
        tokio::select! {
            _ = &mut first => panic!("fixture inference should not be instant"),
            _ = tokio::time::sleep(Duration::from_millis(1)) => {}
        }
        let queued = worker.synthesize(request("second", VoiceIntent::Default));
        tokio::pin!(queued);
        tokio::select! {
            _ = &mut queued => panic!("queued inference should wait"),
            _ = tokio::time::sleep(Duration::from_millis(1)) => {}
        }
        assert!(matches!(
            worker
                .synthesize(request("third", VoiceIntent::Default))
                .await,
            Err(SophonError::ResourceLimit(_))
        ));
        assert!(first.await.is_ok());
        assert!(queued.await.is_ok());
        assert!(matches!(
            worker
                .synthesize(request("oversize", VoiceIntent::Default))
                .await,
            Err(SophonError::ResourceLimit(_))
        ));
    }
}
