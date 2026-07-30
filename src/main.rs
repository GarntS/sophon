//! Daemon composition and provider-handle startup.

use std::sync::Arc;

use sophon::{
    config::{
        Config, ConfigPaths, DEFAULT_MAX_AUDIO_BYTES, DEFAULT_MAX_AUDIO_SECONDS, DEFAULT_MODEL_ID,
        DEFAULT_STT_PROVIDER, DEFAULT_TTS_MODEL_ID, DEFAULT_TTS_PROVIDER, Quantization, TtsConfig,
    },
    dbus::{
        SophonDbus,
        transport::{BUS_NAME, OBJECT_PATH},
    },
    error::SophonError,
    model_registry::{
        LoaderKind, ModelRegistry, common_model_root, package_registry_path, require_roles,
    },
    provider_runtime::{SttProviderHandle, TtsProviderHandle},
    stt::{STTService, STTWorker, TranscriptionOptions, backend},
    tts::{
        TtsCapabilities, TtsProviderModel, TtsService, TtsWorker, create_tts_provider,
        playback::{CpalPlayback, PlaybackWorker},
    },
};

#[cfg(any(feature = "qwen-cpu", feature = "qwen-cuda", feature = "qwen-vulkan"))]
fn install_qwen_log_bridge() {
    qwentts_cpp::set_log_callback(Some(Arc::new(|level, message| match level {
        qwentts_cpp::LogLevel::Debug => tracing::debug!(target: "qwentts_cpp", "{message}"),
        qwentts_cpp::LogLevel::Info => tracing::info!(target: "qwentts_cpp", "{message}"),
        qwentts_cpp::LogLevel::Warning => tracing::warn!(target: "qwentts_cpp", "{message}"),
        qwentts_cpp::LogLevel::Error => tracing::error!(target: "qwentts_cpp", "{message}"),
    })));
}

#[cfg(not(any(feature = "qwen-cpu", feature = "qwen-cuda", feature = "qwen-vulkan")))]
fn install_qwen_log_bridge() {}

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
    let (engine, roles): (_, &[&str]) = match metadata.kind {
        LoaderKind::Parakeet => (
            sophon::config::Engine::Parakeet,
            &["encoder", "decoder_joint", "nemo", "vocabulary"],
        ),
        LoaderKind::Canary => (
            sophon::config::Engine::Canary,
            &["encoder", "decoder", "nemo", "vocabulary"],
        ),
        kind => {
            return Err(SophonError::ModelUnavailable(format!(
                "unsupported STT registry kind `{kind:?}`"
            )));
        }
    };
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
            require_roles(&paths, &["model", "voices"], &identity)
                .map_err(|error| SophonError::ModelUnavailable(error.to_string()))?;
            TtsProviderModel::KokoroDirectory(
                common_model_root(&paths, &identity)
                    .map_err(|error| SophonError::ModelUnavailable(error.to_string()))?,
            )
        }
        LoaderKind::Base | LoaderKind::CustomVoice | LoaderKind::VoiceDesign => {
            require_roles(&paths, &["talker", "codec"], &identity)
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
    let registry = ModelRegistry::initialize_global(ModelRegistry::from_path(
        &package_registry_path(),
        paths.model_cache.clone(),
        reqwest::Client::new(),
    )?)?;
    let config = Config::load_with_catalog(&paths, registry.catalog());
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
