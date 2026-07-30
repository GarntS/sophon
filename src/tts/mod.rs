//! Provider-neutral text-to-speech engine contract and scheduling.

pub mod playback;
#[cfg(any(feature = "qwen-cpu", feature = "qwen-cuda", feature = "qwen-vulkan"))]
mod qwen;
pub mod service;
pub mod types;
mod worker;

#[cfg(any(feature = "qwen-cpu", feature = "qwen-cuda", feature = "qwen-vulkan"))]
pub use qwen::{
    QwenEngineAdapter, QwenTtsBaseProvider, QwenTtsCustomVoiceProvider, QwenTtsVoiceDesignProvider,
    normalize_qwen_language,
};
pub use service::TtsService;
pub use types::{TtsCapabilities, TtsRequest, TtsStreamControl, TtsStreamEvent, VoiceIntent};
pub use worker::{TtsStream, TtsWorker};

use std::path::{Path, PathBuf};

use tts_rs::{
    SynthesisEngine,
    engines::kokoro::{KokoroEngine, KokoroInferenceParams, KokoroModelParams},
};

use crate::{
    audio::OwnedAudio,
    config::{TtsConfig, TtsProviderConfig},
    error::SophonError,
    model_registry::LoaderKind,
};
pub trait TtsProvider: Send {
    fn provider_id(&self) -> &'static str;
    fn model_id(&self) -> &str;
    fn capabilities(&self) -> TtsCapabilities;
    fn voices(&self) -> &[String];
    fn synthesize(&mut self, request: &TtsRequest) -> Result<OwnedAudio, SophonError>;

    fn supports_streaming(&self) -> bool {
        false
    }

    fn synthesize_streaming(
        &mut self,
        _request: &TtsRequest,
        _emit: &mut (dyn FnMut(TtsStreamEvent) -> TtsStreamControl + Send),
    ) -> Result<(), SophonError> {
        Err(SophonError::UnsupportedCapability(
            "provider does not support streaming synthesis".into(),
        ))
    }
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
            sampling: crate::config::QwenSamplingConfig::default(),
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
}
