//! Provider-owned observable model state.

use std::sync::{Arc, RwLock};

use crate::{
    model_registry::{ModelRegistry, ResolutionStatus},
    stt::STTService,
    tts::{TtsCapabilities, TtsService},
};

/// Immutable state value exposed to transports.
#[derive(Debug, Clone, PartialEq)]
pub enum ModelState {
    Initializing,
    Downloading { progress: f32 },
    Loading,
    Ready,
    Failed { message: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProviderSnapshot {
    pub state: ModelState,
    pub active_provider: String,
    pub active_model: String,
    pub download_progress: f32,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuntimePhase {
    Initializing,
    Resolving,
    Loading,
    Ready,
    Failed,
}

struct SttRuntime {
    phase: RuntimePhase,
    failure: Option<String>,
    service: Option<Arc<STTService>>,
}

/// Long-lived STT handle. It exists before initialization and owns the ready
/// service (and therefore its worker) after loading succeeds.
#[derive(Clone)]
pub struct SttProviderHandle {
    registry: Arc<ModelRegistry>,
    provider: Arc<str>,
    model: Arc<str>,
    runtime: Arc<RwLock<SttRuntime>>,
}

impl std::fmt::Debug for SttProviderHandle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SttProviderHandle")
            .field("provider", &self.provider)
            .field("model", &self.model)
            .field("state", &self.state())
            .finish()
    }
}

impl SttProviderHandle {
    pub fn new(
        registry: Arc<ModelRegistry>,
        provider: impl Into<Arc<str>>,
        model: impl Into<Arc<str>>,
    ) -> Self {
        Self {
            registry,
            provider: provider.into(),
            model: model.into(),
            runtime: Arc::new(RwLock::new(SttRuntime {
                phase: RuntimePhase::Initializing,
                failure: None,
                service: None,
            })),
        }
    }

    pub fn provider(&self) -> &str {
        &self.provider
    }
    pub fn model(&self) -> &str {
        &self.model
    }
    pub fn registry(&self) -> &Arc<ModelRegistry> {
        &self.registry
    }

    pub fn begin_resolution(&self) {
        self.runtime
            .write()
            .expect("STT provider lock poisoned")
            .phase = RuntimePhase::Resolving;
    }

    pub fn loading(&self) {
        self.runtime
            .write()
            .expect("STT provider lock poisoned")
            .phase = RuntimePhase::Loading;
    }

    pub fn ready(&self, service: Arc<STTService>) {
        let mut runtime = self.runtime.write().expect("STT provider lock poisoned");
        runtime.service = Some(service);
        runtime.failure = None;
        runtime.phase = RuntimePhase::Ready;
    }

    pub fn failed(&self, error: impl Into<String>) {
        let mut runtime = self.runtime.write().expect("STT provider lock poisoned");
        runtime.failure = Some(error.into());
        runtime.service = None;
        runtime.phase = RuntimePhase::Failed;
    }

    pub fn service(&self) -> Option<Arc<STTService>> {
        self.runtime
            .read()
            .expect("STT provider lock poisoned")
            .service
            .clone()
    }

    pub fn state(&self) -> ModelState {
        let runtime = self.runtime.read().expect("STT provider lock poisoned");
        state_from_runtime(
            runtime.phase,
            runtime.failure.as_deref(),
            &self.registry,
            &self.provider,
            &self.model,
        )
    }

    pub fn snapshot(&self) -> ProviderSnapshot {
        snapshot(self.state(), &self.provider, &self.model)
    }
}

struct TtsRuntime {
    phase: RuntimePhase,
    failure: Option<String>,
    service: Option<Arc<TtsService>>,
    voices: Vec<String>,
    capabilities: TtsCapabilities,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TtsProviderSnapshot {
    pub provider: ProviderSnapshot,
    pub available_voices: Vec<String>,
    pub capabilities: TtsCapabilities,
}

/// Long-lived TTS handle with provider runtime capabilities and worker owner.
#[derive(Clone)]
pub struct TtsProviderHandle {
    registry: Arc<ModelRegistry>,
    provider: Arc<str>,
    model: Arc<str>,
    runtime: Arc<RwLock<TtsRuntime>>,
}

impl std::fmt::Debug for TtsProviderHandle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TtsProviderHandle")
            .field("provider", &self.provider)
            .field("model", &self.model)
            .field("state", &self.state())
            .finish()
    }
}

impl TtsProviderHandle {
    pub fn new(
        registry: Arc<ModelRegistry>,
        provider: impl Into<Arc<str>>,
        model: impl Into<Arc<str>>,
    ) -> Self {
        Self {
            registry,
            provider: provider.into(),
            model: model.into(),
            runtime: Arc::new(RwLock::new(TtsRuntime {
                phase: RuntimePhase::Initializing,
                failure: None,
                service: None,
                voices: Vec::new(),
                capabilities: TtsCapabilities {
                    named_voices: false,
                    voice_cloning: false,
                    voice_design: false,
                    speed_control: false,
                },
            })),
        }
    }

    pub fn provider(&self) -> &str {
        &self.provider
    }
    pub fn model(&self) -> &str {
        &self.model
    }
    pub fn registry(&self) -> &Arc<ModelRegistry> {
        &self.registry
    }

    pub fn begin_resolution(&self) {
        self.runtime
            .write()
            .expect("TTS provider lock poisoned")
            .phase = RuntimePhase::Resolving;
    }

    pub fn loading(&self) {
        self.runtime
            .write()
            .expect("TTS provider lock poisoned")
            .phase = RuntimePhase::Loading;
    }

    pub fn ready(
        &self,
        service: Arc<TtsService>,
        voices: Vec<String>,
        capabilities: TtsCapabilities,
    ) {
        let mut runtime = self.runtime.write().expect("TTS provider lock poisoned");
        runtime.service = Some(service);
        runtime.voices = voices;
        runtime.capabilities = capabilities;
        runtime.failure = None;
        runtime.phase = RuntimePhase::Ready;
    }

    pub fn failed(&self, error: impl Into<String>) {
        let mut runtime = self.runtime.write().expect("TTS provider lock poisoned");
        runtime.failure = Some(error.into());
        runtime.service = None;
        runtime.phase = RuntimePhase::Failed;
    }

    pub fn service(&self) -> Option<Arc<TtsService>> {
        self.runtime
            .read()
            .expect("TTS provider lock poisoned")
            .service
            .clone()
    }

    pub fn state(&self) -> ModelState {
        let runtime = self.runtime.read().expect("TTS provider lock poisoned");
        state_from_runtime(
            runtime.phase,
            runtime.failure.as_deref(),
            &self.registry,
            &self.provider,
            &self.model,
        )
    }

    pub fn snapshot(&self) -> TtsProviderSnapshot {
        let runtime = self.runtime.read().expect("TTS provider lock poisoned");
        TtsProviderSnapshot {
            provider: snapshot(
                state_from_runtime(
                    runtime.phase,
                    runtime.failure.as_deref(),
                    &self.registry,
                    &self.provider,
                    &self.model,
                ),
                &self.provider,
                &self.model,
            ),
            available_voices: runtime.voices.clone(),
            capabilities: runtime.capabilities,
        }
    }
}

fn state_from_runtime(
    phase: RuntimePhase,
    failure: Option<&str>,
    registry: &ModelRegistry,
    provider: &str,
    model: &str,
) -> ModelState {
    match phase {
        RuntimePhase::Initializing => ModelState::Initializing,
        RuntimePhase::Resolving => match registry.status(provider, model) {
            ResolutionStatus::Pending => ModelState::Downloading { progress: 0.0 },
            ResolutionStatus::Downloading { progress } => ModelState::Downloading { progress },
            ResolutionStatus::Ready => ModelState::Loading,
            ResolutionStatus::Failed { message } => ModelState::Failed { message },
        },
        RuntimePhase::Loading => ModelState::Loading,
        RuntimePhase::Ready => ModelState::Ready,
        RuntimePhase::Failed => ModelState::Failed {
            message: failure
                .unwrap_or("provider initialization failed")
                .to_owned(),
        },
    }
}

fn snapshot(state: ModelState, provider: &str, model: &str) -> ProviderSnapshot {
    let download_progress = match state {
        ModelState::Downloading { progress } => progress.clamp(0.0, 1.0),
        ModelState::Loading | ModelState::Ready => 1.0,
        ModelState::Initializing | ModelState::Failed { .. } => 0.0,
    };
    let last_error = match &state {
        ModelState::Failed { message } => Some(message.clone()),
        _ => None,
    };
    ProviderSnapshot {
        state,
        active_provider: provider.to_owned(),
        active_model: model.to_owned(),
        download_progress,
        last_error,
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, path::PathBuf};

    use crate::model_registry::{Language, LoaderKind, ModelCatalog, ModelFile, ModelManifest};

    use super::*;

    fn registry() -> Arc<ModelRegistry> {
        let manifest = ModelManifest {
            kind: LoaderKind::Parakeet,
            revision: "fixture".into(),
            languages: vec![Language::En],
            files: BTreeMap::from([(
                "model".into(),
                ModelFile {
                    path: PathBuf::from("model.bin"),
                    url: "http://127.0.0.1:9/unused".into(),
                    sha256: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                        .into(),
                    size: 1,
                },
            )]),
        };
        Arc::new(ModelRegistry::new(
            ModelCatalog {
                providers: BTreeMap::from([(
                    "fixture".into(),
                    BTreeMap::from([("model".into(), manifest)]),
                )]),
            },
            tempfile::tempdir().unwrap().keep(),
            reqwest::Client::new(),
        ))
    }

    #[tokio::test]
    async fn stt_state_covers_initializing_registry_failure_loading_and_native_failure() {
        let registry = registry();
        let unknown = SttProviderHandle::new(Arc::clone(&registry), "fixture", "missing");
        assert_eq!(unknown.state(), ModelState::Initializing);
        unknown.begin_resolution();
        assert_eq!(unknown.state(), ModelState::Downloading { progress: 0.0 });
        assert!(registry.resolve("fixture", "missing").await.is_err());
        assert!(matches!(unknown.state(), ModelState::Failed { .. }));

        let loading = SttProviderHandle::new(registry, "fixture", "model");
        loading.loading();
        assert_eq!(loading.state(), ModelState::Loading);
        loading.failed("native loader failed");
        assert_eq!(
            loading.state(),
            ModelState::Failed {
                message: "native loader failed".into()
            }
        );
    }

    #[test]
    fn tts_runtime_capabilities_and_provider_outcomes_are_independent() {
        let registry = registry();
        let stt = SttProviderHandle::new(Arc::clone(&registry), "fixture", "model");
        let tts = TtsProviderHandle::new(registry, "fixture", "model");
        stt.failed("STT failed");
        {
            let mut runtime = tts.runtime.write().unwrap();
            runtime.phase = RuntimePhase::Ready;
            runtime.voices = vec!["voice".into()];
            runtime.capabilities.named_voices = true;
        }
        assert!(matches!(stt.state(), ModelState::Failed { .. }));
        assert_eq!(tts.state(), ModelState::Ready);
        assert_eq!(tts.snapshot().available_voices, ["voice"]);
        assert!(tts.snapshot().capabilities.named_voices);
    }
}
