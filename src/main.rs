//! Daemon composition and provider-handle startup.

use std::path::PathBuf;
use std::sync::Arc;

use sophon::{
    config::{
        Config, ConfigError, ConfigPaths, DEFAULT_MAX_AUDIO_BYTES, DEFAULT_MAX_AUDIO_SECONDS,
        DEFAULT_MODEL_ID, DEFAULT_STT_PROVIDER, DEFAULT_TTS_MODEL_ID, DEFAULT_TTS_PROVIDER,
        Quantization, TtsConfig,
    },
    dbus::{
        SophonDbus,
        transport::{BUS_NAME, OBJECT_PATH},
    },
    error::SophonError,
    model_registry::{
        LoaderKind, ModelCatalog, ModelRegistry, common_model_root, package_registry_path,
        require_roles,
    },
    provider_runtime::{SttProviderHandle, TtsProviderHandle},
    stt::{STTService, STTWorker, TranscriptionOptions, backend},
    tts::{
        TtsCapabilities, TtsProviderModel, TtsService, TtsWorker, create_tts_provider,
        playback::{CpalPlayback, PlaybackWorker},
    },
};

fn install_qwen_log_bridge() {
    qwentts_cpp::set_log_callback(Some(Arc::new(|level, message| match level {
        qwentts_cpp::LogLevel::Debug => tracing::debug!(target: "qwentts_cpp", "{message}"),
        qwentts_cpp::LogLevel::Info => tracing::info!(target: "qwentts_cpp", "{message}"),
        qwentts_cpp::LogLevel::Warning => tracing::warn!(target: "qwentts_cpp", "{message}"),
        qwentts_cpp::LogLevel::Error => tracing::error!(target: "qwentts_cpp", "{message}"),
    })));
}

/// Selects the cache root for the process-global registry from validated
/// configuration, falling back to the inert XDG-derived root only when
/// configuration is invalid so that no model resolution is started until
/// configuration succeeds. Kept pure and allocation-free to stay testable
/// without exposing registry internals.
fn registry_cache_root(
    config: &Result<Config, ConfigError>,
    default_cache: &std::path::Path,
) -> PathBuf {
    match config {
        Ok(config) => config.cache_dir.clone(),
        Err(_) => default_cache.to_owned(),
    }
}

fn main() {
    let runtime = tokio::runtime::Runtime::new().expect("failed to create Tokio runtime");
    if let Err(error) = runtime.block_on(run_async()) {
        eprintln!("sophon: {error}");
    }
}

async fn initialize(
    config: Config,
    handle: SttProviderHandle,
) -> Result<TranscriptionOptions, SophonError> {
    let metadata = handle
        .registry()
        .metadata(handle.provider(), handle.model())
        .map_err(|error| SophonError::ModelUnavailable(error.to_string()))?
        .clone();
    let engine = match metadata.kind {
        LoaderKind::Parakeet => sophon::config::Engine::Parakeet,
        LoaderKind::Canary => sophon::config::Engine::Canary,
        kind => {
            return Err(SophonError::ModelUnavailable(format!(
                "unsupported STT registry kind `{kind:?}`"
            )));
        }
    };
    let roles = metadata.kind.required_roles();
    handle.begin_resolution();
    let paths = handle
        .registry()
        .resolve(handle.provider(), handle.model())
        .await
        .map_err(|error| SophonError::ModelUnavailable(error.to_string()))?;
    let identity = format!("{}/{}", handle.provider(), handle.model());
    require_roles(&paths, roles, &identity)
        .map_err(|error| SophonError::ModelUnavailable(error.to_string()))?;
    let model_dir = common_model_root(&paths, &identity)
        .map_err(|error| SophonError::ModelUnavailable(error.to_string()))?;
    backend::configure_accelerator(config.accelerator)?;
    handle.loading();
    let model = backend::create_model(engine, &model_dir, Quantization::Int8)?;
    let model_sample_rate = model.capabilities().sample_rate;
    if model_sample_rate == 0 {
        return Err(SophonError::ModelUnavailable(
            "loaded STT model advertises a zero sample rate".into(),
        ));
    }
    let worker = STTWorker::new(model, config.queue_capacity);
    let defaults = TranscriptionOptions {
        language: Some(config.language),
    };
    let supported_languages = metadata
        .languages
        .iter()
        .map(|language| language.as_str().to_owned())
        .collect();
    handle.ready(Arc::new(STTService::new(
        worker,
        defaults.clone(),
        supported_languages,
        model_sample_rate,
        config.max_audio_seconds,
    )));
    Ok(defaults)
}

async fn initialize_tts(
    config: TtsConfig,
    handle: TtsProviderHandle,
) -> Result<(Vec<String>, TtsCapabilities), SophonError> {
    let metadata = handle
        .registry()
        .metadata(handle.provider(), handle.model())
        .map_err(|error| SophonError::ModelUnavailable(error.to_string()))?
        .clone();
    handle.begin_resolution();
    let paths = handle
        .registry()
        .resolve(handle.provider(), handle.model())
        .await
        .map_err(|error| SophonError::ModelUnavailable(error.to_string()))?;
    let identity = format!("{}/{}", handle.provider(), handle.model());
    let provider_model = match metadata.kind {
        LoaderKind::Kokoro => {
            require_roles(&paths, metadata.kind.required_roles(), &identity)
                .map_err(|error| SophonError::ModelUnavailable(error.to_string()))?;
            TtsProviderModel::KokoroDirectory(
                common_model_root(&paths, &identity)
                    .map_err(|error| SophonError::ModelUnavailable(error.to_string()))?,
            )
        }
        LoaderKind::Base | LoaderKind::CustomVoice | LoaderKind::VoiceDesign => {
            require_roles(&paths, metadata.kind.required_roles(), &identity)
                .map_err(|error| SophonError::ModelUnavailable(error.to_string()))?;
            TtsProviderModel::Qwen {
                model_id: handle.model().to_owned(),
                kind: metadata.kind,
                talker_path: paths["talker"].clone(),
                codec_path: paths["codec"].clone(),
            }
        }
        kind => {
            return Err(SophonError::ModelUnavailable(format!(
                "unsupported TTS registry kind `{kind:?}`"
            )));
        }
    };
    handle.loading();
    let optimized_dir = config.operational.cache_dir.join("optimized");
    std::fs::create_dir_all(&optimized_dir)
        .map_err(|error| SophonError::ModelUnavailable(error.to_string()))?;
    let provider_config = config.clone();
    let provider = tokio::task::spawn_blocking(move || {
        create_tts_provider(
            &provider_config,
            provider_model,
            Some(optimized_dir.join("kokoro-v1.0-int8.optimized.onnx")),
        )
    })
    .await
    .map_err(|error| SophonError::ModelUnavailable(format!("TTS load task failed: {error}")))??;
    let voices = provider.voices().to_vec();
    let capabilities = provider.capabilities();
    let worker = TtsWorker::new(
        provider,
        config.operational.queue_capacity,
        config.operational.max_generated_audio_seconds,
    );
    let playback = PlaybackWorker::new(Box::new(CpalPlayback), config.operational.queue_capacity);
    let supported_languages = metadata
        .languages
        .iter()
        .map(|language| language.as_str().to_owned())
        .collect();
    let service = Arc::new(TtsService::new(
        worker,
        playback,
        config,
        supported_languages,
    ));
    handle.ready(service, voices.clone(), capabilities);
    Ok((voices, capabilities))
}

async fn run_async() -> Result<(), Box<dyn std::error::Error>> {
    install_qwen_log_bridge();
    let paths = ConfigPaths::discover()?;
    let package_path = package_registry_path();
    let catalog = ModelCatalog::load(&package_path)?;
    let config = Config::load_with_catalog(&paths, &catalog);
    let registry = ModelRegistry::initialize_global(ModelRegistry::new(
        catalog,
        registry_cache_root(&config, &paths.model_cache),
        reqwest::Client::new(),
    ))?;
    let (stt_provider, stt_model, tts_provider, tts_model) = match &config {
        Ok(config) => match &config.tts {
            Ok(tts) => (
                config.provider.as_str(),
                config.model_id.as_str(),
                tts.provider_id(),
                tts.model_id(),
            ),
            Err(_) => (
                config.provider.as_str(),
                config.model_id.as_str(),
                DEFAULT_TTS_PROVIDER,
                DEFAULT_TTS_MODEL_ID,
            ),
        },
        Err(_) => (
            DEFAULT_STT_PROVIDER,
            DEFAULT_MODEL_ID,
            DEFAULT_TTS_PROVIDER,
            DEFAULT_TTS_MODEL_ID,
        ),
    };
    let stt_handle = SttProviderHandle::new(Arc::clone(&registry), stt_provider, stt_model);
    let tts_handle = TtsProviderHandle::new(Arc::clone(&registry), tts_provider, tts_model);

    let connection = zbus::connection::Builder::session()?
        .name(BUS_NAME)?
        .serve_at(
            OBJECT_PATH,
            SophonDbus::unavailable(
                TranscriptionOptions {
                    language: Some("en".into()),
                },
                stt_handle.clone(),
                tts_handle.clone(),
                DEFAULT_MAX_AUDIO_BYTES,
                DEFAULT_MAX_AUDIO_SECONDS,
            ),
        )?
        .build()
        .await?;
    let interface = connection
        .object_server()
        .interface::<_, SophonDbus>(OBJECT_PATH)
        .await?;

    match config {
        Err(error) => {
            stt_handle.failed(error.to_string());
            tts_handle.failed(error.to_string());
        }
        Ok(config) => {
            let stt_config = config.clone();
            let stt_interface = interface.clone();
            let stt = stt_handle.clone();
            tokio::spawn(async move {
                let max_audio_bytes = stt_config.max_audio_bytes;
                let max_audio_seconds = stt_config.max_audio_seconds;
                match initialize(stt_config, stt.clone()).await {
                    Ok(defaults) => stt_interface.get_mut().await.install(
                        defaults,
                        max_audio_bytes,
                        max_audio_seconds,
                    ),
                    Err(error) => stt.failed(error.to_string()),
                }
            });

            match config.tts {
                Err(error) => tts_handle.failed(error),
                Ok(tts_config) => {
                    interface.get_mut().await.install_tts(tts_config.clone());
                    let tts = tts_handle.clone();
                    tokio::spawn(async move {
                        if let Err(error) = initialize_tts(tts_config, tts.clone()).await {
                            tts.failed(error.to_string());
                        }
                    });
                }
            }
        }
    }

    let stt_observer = interface.clone();
    tokio::spawn(async move {
        let mut previous = stt_handle.snapshot();
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            let current = stt_handle.snapshot();
            if current != previous {
                previous = current;
                let _ = SophonDbus::emit_lifecycle_changed(&stt_observer).await;
            }
        }
    });
    let tts_observer = interface;
    tokio::spawn(async move {
        let mut previous = tts_handle.snapshot();
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            let current = tts_handle.snapshot();
            if current != previous {
                previous = current;
                let _ = SophonDbus::emit_tts_lifecycle_changed(&tts_observer).await;
            }
        }
    });
    tokio::signal::ctrl_c().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sophon::model_registry::ModelCatalog;
    use std::fs;

    fn catalog() -> ModelCatalog {
        ModelCatalog::from_yaml(include_str!("../model_registry.yaml")).unwrap()
    }

    fn fixture(contents: Option<&str>) -> (tempfile::TempDir, ConfigPaths) {
        let root = tempfile::tempdir().unwrap();
        let paths = ConfigPaths::from_homes(root.path().join("config"), root.path().join("cache"));
        if let Some(contents) = contents {
            fs::create_dir_all(paths.config_file.parent().unwrap()).unwrap();
            fs::write(&paths.config_file, contents).unwrap();
        }
        (root, paths)
    }

    #[test]
    fn valid_cache_override_is_passed_to_registry_construction() {
        let (_root, paths) = fixture(Some("cache_dir: /opt/sophon-cache\n"));
        let config = Config::load_with_catalog(&paths, &catalog());
        assert!(config.is_ok());
        assert_eq!(
            registry_cache_root(&config, &paths.model_cache),
            PathBuf::from("/opt/sophon-cache")
        );
    }

    #[test]
    fn omitted_cache_override_uses_the_xdg_model_cache() {
        let (_root, paths) = fixture(None);
        let config = Config::load_with_catalog(&paths, &catalog()).unwrap();
        assert_eq!(config.cache_dir, paths.model_cache);
        assert_eq!(
            registry_cache_root(&Ok(config), &paths.model_cache),
            paths.model_cache
        );
    }

    #[test]
    fn invalid_configuration_selects_the_inert_root_and_starts_no_resolution() {
        let (_root, paths) = fixture(Some("provider: transcribe-rs\nmodel_id: missing\n"));
        let config = Config::load_with_catalog(&paths, &catalog());
        // Strict configuration fails; the helper keeps the inert XDG root so
        // `run_async` constructs the registry only for the unavailable lifecycle
        // and never reaches `initialize`/`initialize_tts`.
        assert!(config.is_err());
        assert_eq!(
            registry_cache_root(&config, &paths.model_cache),
            paths.model_cache
        );
    }
}
