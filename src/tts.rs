//! Provider-neutral text-to-speech engine contract and scheduling.

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

use crate::domain::{OwnedAudio, SophonError, TtsCapabilities, TtsRequest, VoiceIntent};

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
