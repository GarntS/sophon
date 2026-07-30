use super::{
    TtsCapabilities, TtsProvider, TtsRequest, TtsStreamControl, TtsStreamEvent, VoiceIntent,
    validate_capabilities,
};
use crate::{audio::OwnedAudio, config::QwenSamplingConfig, error::SophonError};
use qwentts_cpp::{
    Language as QwenLanguage, QwenTtsEngine, SamplingOptions as QwenSamplingOptions,
    StreamChunk as QwenStreamChunk, StreamControl as QwenStreamControl, Voice as QwenVoice,
    VoiceReference as QwenVoiceReference,
};
use std::path::Path;

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
    if value
        .chars()
        .any(|c| c == '\0' || (c.is_control() && !c.is_whitespace()))
    {
        return Err(SophonError::InvalidTtsOptions(format!(
            "{field} contains a disallowed control character"
        )));
    }
    Ok(())
}

pub fn normalize_qwen_language(language: Option<&str>) -> Result<QwenLanguage, SophonError> {
    let Some(language) = language else {
        return Ok(QwenLanguage::Auto);
    };
    match language.to_ascii_lowercase().as_str() {
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

pub struct QwenEngineAdapter {
    engine: QwenTtsEngine,
    sampling: QwenSamplingOptions,
}
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
            .map_err(|e| SophonError::ModelUnavailable(e.to_string()))?;
        let tokens = engine
            .duration_sec_to_tokens(max_generated_audio_seconds as f32)
            .map_err(|e| SophonError::ModelUnavailable(e.to_string()))?;
        Ok(Self {
            engine,
            sampling: Self::effective_sampling(sampling, tokens)?,
        })
    }
    fn effective_sampling(
        c: &QwenSamplingConfig,
        duration_tokens: u32,
    ) -> Result<QwenSamplingOptions, SophonError> {
        Ok(QwenSamplingOptions {
            seed: c
                .seed
                .map(i64::try_from)
                .transpose()
                .map_err(|_| SophonError::ModelUnavailable("Qwen seed exceeds i64::MAX".into()))?,
            max_new_tokens: c.max_new_tokens.min(duration_tokens),
            temperature: c.temperature,
            top_k: c.top_k,
            top_p: c.top_p,
            repetition_penalty: c.repetition_penalty,
        })
    }
    pub fn speakers(&self) -> Result<Vec<String>, SophonError> {
        self.engine
            .speakers()
            .map_err(|e| SophonError::ModelUnavailable(e.to_string()))
    }
    pub fn extract_voice_reference(
        &mut self,
        samples: &[f32],
    ) -> Result<QwenVoiceReference, SophonError> {
        self.engine
            .extract_voice_reference(samples)
            .map_err(|e| SophonError::SynthesisFailed(e.to_string()))
    }
    pub fn synthesize(
        &mut self,
        text: &str,
        language: QwenLanguage,
        voice: QwenVoice<'_>,
    ) -> Result<OwnedAudio, SophonError> {
        let audio = self
            .engine
            .synthesize(
                text,
                Some(qwentts_cpp::SynthesisOptions {
                    language,
                    sampling: self.sampling.clone(),
                    voice,
                }),
            )
            .map_err(|e| SophonError::SynthesisFailed(e.to_string()))?;
        if audio.sample_rate != 24_000 {
            return Err(SophonError::SynthesisFailed(format!(
                "qwentts.cpp returned unexpected sample rate {}",
                audio.sample_rate
            )));
        }
        Ok(OwnedAudio {
            samples: audio.samples,
            sample_rate: audio.sample_rate,
        })
    }
    pub fn synthesize_streaming(
        &mut self,
        text: &str,
        language: QwenLanguage,
        voice: QwenVoice<'_>,
        emit: &mut (dyn FnMut(TtsStreamEvent) -> TtsStreamControl + Send),
    ) -> Result<(), SophonError> {
        if emit(TtsStreamEvent::Format {
            sample_rate: 24_000,
        }) == TtsStreamControl::Cancel
        {
            return Err(SophonError::SynthesisFailed(
                "streaming synthesis was cancelled before generation".into(),
            ));
        }
        self.engine
            .synthesize_streaming(
                text,
                Some(qwentts_cpp::SynthesisOptions {
                    language,
                    sampling: self.sampling.clone(),
                    voice,
                }),
                |chunk: QwenStreamChunk| match emit(TtsStreamEvent::Chunk {
                    samples: chunk.samples,
                }) {
                    TtsStreamControl::Continue => QwenStreamControl::Continue,
                    TtsStreamControl::Cancel => QwenStreamControl::Cancel,
                },
            )
            .map_err(|e| SophonError::SynthesisFailed(e.to_string()))
    }
}

enum QwenVoiceRequest<'a> {
    Default,
    Named(&'a str),
    Design(&'a str),
    Clone {
        samples_24khz_mono: &'a [f32],
        transcript: Option<&'a str>,
    },
}
trait QwenProviderEngine: Send {
    fn speakers(&self) -> Result<Vec<String>, SophonError>;
    fn synthesize(
        &mut self,
        text: &str,
        language: QwenLanguage,
        voice: QwenVoiceRequest<'_>,
    ) -> Result<OwnedAudio, SophonError>;
    fn synthesize_streaming(
        &mut self,
        text: &str,
        language: QwenLanguage,
        voice: QwenVoiceRequest<'_>,
        emit: &mut (dyn FnMut(TtsStreamEvent) -> TtsStreamControl + Send),
    ) -> Result<(), SophonError>;
}
impl QwenProviderEngine for QwenEngineAdapter {
    fn speakers(&self) -> Result<Vec<String>, SophonError> {
        QwenEngineAdapter::speakers(self)
    }
    fn synthesize(
        &mut self,
        text: &str,
        language: QwenLanguage,
        voice: QwenVoiceRequest<'_>,
    ) -> Result<OwnedAudio, SophonError> {
        self.execute(text, language, voice, None)
    }
    fn synthesize_streaming(
        &mut self,
        text: &str,
        language: QwenLanguage,
        voice: QwenVoiceRequest<'_>,
        emit: &mut (dyn FnMut(TtsStreamEvent) -> TtsStreamControl + Send),
    ) -> Result<(), SophonError> {
        self.execute_streaming(text, language, voice, emit)
    }
}
impl QwenEngineAdapter {
    fn execute(
        &mut self,
        text: &str,
        language: QwenLanguage,
        voice: QwenVoiceRequest<'_>,
        _emit: Option<()>,
    ) -> Result<OwnedAudio, SophonError> {
        match voice {
            QwenVoiceRequest::Default => self.synthesize(text, language, QwenVoice::Default),
            QwenVoiceRequest::Named(v) => self.synthesize(text, language, QwenVoice::Named(v)),
            QwenVoiceRequest::Design(v) => self.synthesize(text, language, QwenVoice::Design(v)),
            QwenVoiceRequest::Clone {
                samples_24khz_mono,
                transcript,
            } => {
                let r = self.extract_voice_reference(samples_24khz_mono)?;
                self.synthesize(
                    text,
                    language,
                    match transcript {
                        Some(t) => QwenVoice::CloneWithTranscript {
                            reference: &r,
                            transcript: t,
                        },
                        None => QwenVoice::Clone(&r),
                    },
                )
            }
        }
    }
    fn execute_streaming(
        &mut self,
        text: &str,
        language: QwenLanguage,
        voice: QwenVoiceRequest<'_>,
        emit: &mut (dyn FnMut(TtsStreamEvent) -> TtsStreamControl + Send),
    ) -> Result<(), SophonError> {
        match voice {
            QwenVoiceRequest::Default => {
                self.synthesize_streaming(text, language, QwenVoice::Default, emit)
            }
            QwenVoiceRequest::Named(v) => {
                self.synthesize_streaming(text, language, QwenVoice::Named(v), emit)
            }
            QwenVoiceRequest::Design(v) => {
                self.synthesize_streaming(text, language, QwenVoice::Design(v), emit)
            }
            QwenVoiceRequest::Clone {
                samples_24khz_mono,
                transcript,
            } => {
                let r = self.extract_voice_reference(samples_24khz_mono)?;
                self.synthesize_streaming(
                    text,
                    language,
                    match transcript {
                        Some(t) => QwenVoice::CloneWithTranscript {
                            reference: &r,
                            transcript: t,
                        },
                        None => QwenVoice::Clone(&r),
                    },
                    emit,
                )
            }
        }
    }
}

pub struct QwenTtsBaseProvider {
    engine: Box<dyn QwenProviderEngine>,
    model_id: String,
    max_text_bytes: u64,
}
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
    fn prepare<'a>(
        &self,
        request: &'a TtsRequest,
    ) -> Result<(QwenLanguage, QwenVoiceRequest<'a>), SophonError> {
        validate_capabilities(self, request)?;
        if request.speed != 1.0 {
            return Err(SophonError::InvalidTtsOptions(
                "Qwen TTS supports only unit speed".into(),
            ));
        }
        validate_qwen_text_like(&request.text, "synthesis text", self.max_text_bytes)?;
        let language = normalize_qwen_language(request.language.as_deref())?;
        let voice = match &request.voice {
            VoiceIntent::Default => QwenVoiceRequest::Default,
            VoiceIntent::Clone {
                reference,
                transcript,
            } => {
                if let Some(t) = transcript {
                    validate_qwen_text_like(t, "clone transcript", self.max_text_bytes)?;
                }
                if reference.sample_rate != 24_000 {
                    return Err(SophonError::InvalidTtsOptions(
                        "Qwen clone references must be 24 kHz mono PCM".into(),
                    ));
                }
                QwenVoiceRequest::Clone {
                    samples_24khz_mono: &reference.samples,
                    transcript: transcript.as_deref(),
                }
            }
            _ => unreachable!("validated above"),
        };
        Ok((language, voice))
    }
}
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
    fn synthesize(&mut self, r: &TtsRequest) -> Result<OwnedAudio, SophonError> {
        let (l, v) = self.prepare(r)?;
        self.engine.synthesize(&r.text, l, v)
    }
    fn supports_streaming(&self) -> bool {
        true
    }
    fn synthesize_streaming(
        &mut self,
        r: &TtsRequest,
        e: &mut (dyn FnMut(TtsStreamEvent) -> TtsStreamControl + Send),
    ) -> Result<(), SophonError> {
        let (l, v) = self.prepare(r)?;
        self.engine.synthesize_streaming(&r.text, l, v, e)
    }
}

pub struct QwenTtsCustomVoiceProvider {
    engine: Box<dyn QwenProviderEngine>,
    model_id: String,
    voices: Vec<String>,
    default_voice: String,
    max_text_bytes: u64,
}
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
        if !voices.iter().any(|v| v == &default_voice) {
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
    fn prepare<'a>(
        max_text_bytes: u64,
        default_voice: &'a str,
        r: &'a TtsRequest,
    ) -> Result<(QwenLanguage, QwenVoiceRequest<'a>), SophonError> {
        validate_qwen_text_like(&r.text, "synthesis text", max_text_bytes)?;
        if r.speed != 1.0 {
            return Err(SophonError::InvalidTtsOptions(
                "Qwen TTS supports only unit speed".into(),
            ));
        }
        let l = normalize_qwen_language(r.language.as_deref())?;
        let v = match &r.voice {
            VoiceIntent::Default => default_voice,
            VoiceIntent::Named(v) => v,
            _ => unreachable!("validated above"),
        };
        Ok((l, QwenVoiceRequest::Named(v)))
    }
}
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
    fn synthesize(&mut self, r: &TtsRequest) -> Result<OwnedAudio, SophonError> {
        validate_capabilities(self, r)?;
        let (l, v) = Self::prepare(self.max_text_bytes, &self.default_voice, r)?;
        self.engine.synthesize(&r.text, l, v)
    }
    fn supports_streaming(&self) -> bool {
        true
    }
    fn synthesize_streaming(
        &mut self,
        r: &TtsRequest,
        e: &mut (dyn FnMut(TtsStreamEvent) -> TtsStreamControl + Send),
    ) -> Result<(), SophonError> {
        validate_capabilities(self, r)?;
        let (l, v) = Self::prepare(self.max_text_bytes, &self.default_voice, r)?;
        self.engine.synthesize_streaming(&r.text, l, v, e)
    }
}

pub struct QwenTtsVoiceDesignProvider {
    engine: Box<dyn QwenProviderEngine>,
    model_id: String,
    default_voice_description: String,
    max_text_bytes: u64,
}
impl QwenTtsVoiceDesignProvider {
    pub fn new(
        engine: QwenEngineAdapter,
        model_id: impl Into<String>,
        description: impl Into<String>,
        max: u64,
    ) -> Self {
        Self::with_engine(Box::new(engine), model_id, description, max)
    }
    fn with_engine(
        engine: Box<dyn QwenProviderEngine>,
        model_id: impl Into<String>,
        description: impl Into<String>,
        max: u64,
    ) -> Self {
        Self {
            engine,
            model_id: model_id.into(),
            default_voice_description: description.into(),
            max_text_bytes: max,
        }
    }
    fn prepare<'a>(
        max_text_bytes: u64,
        default_voice_description: &'a str,
        r: &'a TtsRequest,
    ) -> Result<(QwenLanguage, QwenVoiceRequest<'a>), SophonError> {
        if r.speed != 1.0 {
            return Err(SophonError::InvalidTtsOptions(
                "Qwen TTS supports only unit speed".into(),
            ));
        }
        validate_qwen_text_like(&r.text, "synthesis text", max_text_bytes)?;
        let l = normalize_qwen_language(r.language.as_deref())?;
        let d = match &r.voice {
            VoiceIntent::Default => default_voice_description,
            VoiceIntent::Design(d) => d,
            _ => unreachable!("validated above"),
        };
        validate_qwen_text_like(d, "voice description", max_text_bytes)?;
        Ok((l, QwenVoiceRequest::Design(d)))
    }
}
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
    fn synthesize(&mut self, r: &TtsRequest) -> Result<OwnedAudio, SophonError> {
        validate_capabilities(self, r)?;
        let (l, v) = Self::prepare(self.max_text_bytes, &self.default_voice_description, r)?;
        self.engine.synthesize(&r.text, l, v)
    }
    fn supports_streaming(&self) -> bool {
        true
    }
    fn synthesize_streaming(
        &mut self,
        r: &TtsRequest,
        e: &mut (dyn FnMut(TtsStreamEvent) -> TtsStreamControl + Send),
    ) -> Result<(), SophonError> {
        validate_capabilities(self, r)?;
        let (l, v) = Self::prepare(self.max_text_bytes, &self.default_voice_description, r)?;
        self.engine.synthesize_streaming(&r.text, l, v, e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Debug, PartialEq)]
    enum QwenCall {
        Default(QwenLanguage),
        Named(QwenLanguage, String),
        Design(QwenLanguage, String),
        Clone(QwenLanguage, Vec<f32>, Option<String>),
    }

    struct FixtureQwenEngine {
        calls: Arc<Mutex<Vec<QwenCall>>>,
        speakers: Vec<String>,
        fail_once: bool,
    }

    impl FixtureQwenEngine {
        fn call(language: QwenLanguage, voice: QwenVoiceRequest<'_>) -> QwenCall {
            match voice {
                QwenVoiceRequest::Default => QwenCall::Default(language),
                QwenVoiceRequest::Named(speaker) => QwenCall::Named(language, speaker.into()),
                QwenVoiceRequest::Design(description) => {
                    QwenCall::Design(language, description.into())
                }
                QwenVoiceRequest::Clone {
                    samples_24khz_mono,
                    transcript,
                } => QwenCall::Clone(
                    language,
                    samples_24khz_mono.to_vec(),
                    transcript.map(str::to_owned),
                ),
            }
        }

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

    impl QwenProviderEngine for FixtureQwenEngine {
        fn speakers(&self) -> Result<Vec<String>, SophonError> {
            Ok(self.speakers.clone())
        }
        fn synthesize(
            &mut self,
            _: &str,
            language: QwenLanguage,
            voice: QwenVoiceRequest<'_>,
        ) -> Result<OwnedAudio, SophonError> {
            self.respond(Self::call(language, voice))
        }
        fn synthesize_streaming(
            &mut self,
            _: &str,
            language: QwenLanguage,
            voice: QwenVoiceRequest<'_>,
            emit: &mut (dyn FnMut(TtsStreamEvent) -> TtsStreamControl + Send),
        ) -> Result<(), SophonError> {
            self.calls.lock().unwrap().push(Self::call(language, voice));
            if emit(TtsStreamEvent::Format {
                sample_rate: 24_000,
            }) == TtsStreamControl::Cancel
                || emit(TtsStreamEvent::Chunk {
                    samples: vec![0.25],
                }) == TtsStreamControl::Cancel
            {
                return Err(SophonError::SynthesisFailed("fixture cancelled".into()));
            }
            Ok(())
        }
    }

    fn fixture(calls: Arc<Mutex<Vec<QwenCall>>>, speakers: &[&str]) -> FixtureQwenEngine {
        FixtureQwenEngine {
            calls,
            speakers: speakers.iter().map(|speaker| (*speaker).into()).collect(),
            fail_once: false,
        }
    }

    fn request(voice: VoiceIntent) -> TtsRequest {
        TtsRequest {
            text: "hello".into(),
            language: Some("en-US".into()),
            speed: 1.0,
            voice,
        }
    }

    fn synthesize_streaming(
        provider: &mut dyn TtsProvider,
        request: &TtsRequest,
    ) -> Result<(), SophonError> {
        provider.synthesize_streaming(request, &mut |_| TtsStreamControl::Continue)
    }

    #[test]
    fn qwen_dispatches_all_supported_voices_identically_for_buffered_and_streaming() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let mut base =
            QwenTtsBaseProvider::with_engine(Box::new(fixture(calls.clone(), &[])), "base", 1024);
        let mut custom = QwenTtsCustomVoiceProvider::with_engine(
            Box::new(fixture(calls.clone(), &["vivian", "ryan"])),
            "custom",
            "vivian",
            1024,
        )
        .unwrap();
        let mut design = QwenTtsVoiceDesignProvider::with_engine(
            Box::new(fixture(calls.clone(), &[])),
            "design",
            "warm",
            1024,
        );
        let clone = VoiceIntent::Clone {
            reference: OwnedAudio {
                samples: vec![0.1, -0.1],
                sample_rate: 24_000,
            },
            transcript: Some("reference words".into()),
        };
        for request in [request(VoiceIntent::Default), request(clone)] {
            base.synthesize(&request).unwrap();
            synthesize_streaming(&mut base, &request).unwrap();
        }
        for request in [
            request(VoiceIntent::Default),
            request(VoiceIntent::Named("ryan".into())),
        ] {
            custom.synthesize(&request).unwrap();
            synthesize_streaming(&mut custom, &request).unwrap();
        }
        for request in [
            request(VoiceIntent::Default),
            request(VoiceIntent::Design("bright".into())),
        ] {
            design.synthesize(&request).unwrap();
            synthesize_streaming(&mut design, &request).unwrap();
        }
        let calls = calls.lock().unwrap();
        assert_eq!(calls.len(), 12);
        for pair in calls.chunks_exact(2) {
            assert_eq!(pair[0], pair[1]);
        }
        assert_eq!(calls[0], QwenCall::Default(QwenLanguage::English));
        assert_eq!(
            calls[2],
            QwenCall::Clone(
                QwenLanguage::English,
                vec![0.1, -0.1],
                Some("reference words".into())
            )
        );
        assert_eq!(
            calls[4],
            QwenCall::Named(QwenLanguage::English, "vivian".into())
        );
        assert_eq!(
            calls[6],
            QwenCall::Named(QwenLanguage::English, "ryan".into())
        );
        assert_eq!(
            calls[8],
            QwenCall::Design(QwenLanguage::English, "warm".into())
        );
        assert_eq!(
            calls[10],
            QwenCall::Design(QwenLanguage::English, "bright".into())
        );
    }

    #[test]
    fn qwen_validation_errors_are_equal_before_native_work_in_both_modes() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let mut base =
            QwenTtsBaseProvider::with_engine(Box::new(fixture(calls.clone(), &[])), "base", 4);
        let invalid = [
            request(VoiceIntent::Named("nope".into())),
            TtsRequest {
                speed: 1.1,
                ..request(VoiceIntent::Default)
            },
            TtsRequest {
                text: "oversize".into(),
                ..request(VoiceIntent::Default)
            },
            TtsRequest {
                language: Some("ar".into()),
                ..request(VoiceIntent::Default)
            },
            TtsRequest {
                voice: VoiceIntent::Clone {
                    reference: OwnedAudio {
                        samples: vec![0.0],
                        sample_rate: 24_000,
                    },
                    transcript: Some("oversize".into()),
                },
                ..request(VoiceIntent::Default)
            },
            TtsRequest {
                voice: VoiceIntent::Clone {
                    reference: OwnedAudio {
                        samples: vec![0.0],
                        sample_rate: 16_000,
                    },
                    transcript: None,
                },
                ..request(VoiceIntent::Default)
            },
        ];
        for request in invalid {
            let buffered = base.synthesize(&request).unwrap_err();
            let streamed = synthesize_streaming(&mut base, &request).unwrap_err();
            assert_eq!(
                std::mem::discriminant(&buffered),
                std::mem::discriminant(&streamed)
            );
        }
        assert!(calls.lock().unwrap().is_empty());

        let mut custom = QwenTtsCustomVoiceProvider::with_engine(
            Box::new(fixture(calls.clone(), &["vivian"])),
            "custom",
            "vivian",
            4,
        )
        .unwrap();
        let voice = request(VoiceIntent::Named("missing".into()));
        assert_eq!(
            std::mem::discriminant(&custom.synthesize(&voice).unwrap_err()),
            std::mem::discriminant(&synthesize_streaming(&mut custom, &voice).unwrap_err())
        );
        let mut design = QwenTtsVoiceDesignProvider::with_engine(
            Box::new(fixture(calls.clone(), &[])),
            "design",
            "warm",
            4,
        );
        let description = request(VoiceIntent::Design("oversize".into()));
        assert_eq!(
            std::mem::discriminant(&design.synthesize(&description).unwrap_err()),
            std::mem::discriminant(&synthesize_streaming(&mut design, &description).unwrap_err())
        );
        assert!(calls.lock().unwrap().is_empty());
    }

    #[test]
    fn qwen_native_failure_is_request_local() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let mut engine = fixture(calls.clone(), &[]);
        engine.fail_once = true;
        let mut provider = QwenTtsBaseProvider::with_engine(Box::new(engine), "base", 1024);
        let request = request(VoiceIntent::Default);
        assert!(matches!(
            provider.synthesize(&request),
            Err(SophonError::SynthesisFailed(_))
        ));
        assert!(provider.synthesize(&request).is_ok());
        assert_eq!(calls.lock().unwrap().len(), 2);
    }

    #[test]
    fn qwen_language_normalization_is_conservative_and_case_insensitive() {
        assert_eq!(normalize_qwen_language(None).unwrap(), QwenLanguage::Auto);
        assert_eq!(
            normalize_qwen_language(Some("EN-us")).unwrap(),
            QwenLanguage::English
        );
        assert!(matches!(
            normalize_qwen_language(Some("ar")),
            Err(SophonError::InvalidTtsOptions(_))
        ));
    }

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
    }
}
