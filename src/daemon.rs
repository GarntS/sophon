//! Daemon composition and lifecycle startup.

use std::sync::Arc;

use crate::{
    acquisition::{self, ModelLifecycle, ModelLocation, TtsLifecycle, TtsModelLocation},
    backend,
    config::{Config, ConfigPaths, DEFAULT_MAX_AUDIO_BYTES, DEFAULT_MAX_AUDIO_SECONDS, TtsConfig},
    dbus::SophonDbus,
    domain::{SophonError, TranscriptionOptions, TtsCapabilities},
    playback::{PipeWirePlayback, PlaybackWorker},
    postprocess::{IdentityProcessor, PostProcessingPipeline},
    service::{TranscriptionService, TtsService},
    transport::{BUS_NAME, OBJECT_PATH},
    tts::{TtsProviderModel, TtsWorker, create_tts_provider},
    worker::ModelWorker,
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

/// Starts the session-bus service and claims its name before model work begins.
pub fn run() {
    let runtime = tokio::runtime::Runtime::new().expect("failed to create Tokio runtime");
    if let Err(error) = runtime.block_on(run_async()) {
        eprintln!("sophon: {error}");
    }
}

async fn initialize(
    config: Config,
    lifecycle: ModelLifecycle,
) -> Result<(Arc<TranscriptionService>, TranscriptionOptions), SophonError> {
    let definition = acquisition::lookup(&config.model_id).ok_or_else(|| {
        SophonError::ModelUnavailable(format!("unknown model `{}`", config.model_id))
    })?;
    let model_dir =
        match acquisition::resolve_location(&config.model_id, config.model_path.as_deref())? {
            ModelLocation::LocalOverride(path) => path,
            ModelLocation::Registry(model) => {
                if let Some(path) = acquisition::validated_cache(&config.cache_dir, model) {
                    path
                } else if config.automatic_download {
                    let progress = lifecycle.clone();
                    lifecycle.downloading(0.0);
                    acquisition::acquire(&config.cache_dir, model, move |value| {
                        progress.downloading(value)
                    })
                    .await?
                } else {
                    return Err(SophonError::ModelUnavailable(
                        "model is not cached and automatic downloads are disabled".into(),
                    ));
                }
            }
        };
    backend::configure_accelerator(config.accelerator)?;
    lifecycle.loading(definition);
    let model = backend::create_model(config.engine, &model_dir, config.quantization)?;
    let worker = ModelWorker::new(model, config.queue_capacity);
    let defaults = TranscriptionOptions {
        language: Some(config.language),
        translate: Some(config.translate),
    };
    let service = TranscriptionService::new(
        lifecycle.clone(),
        worker,
        defaults.clone(),
        definition.capabilities.clone(),
        PostProcessingPipeline::new(vec![Box::new(IdentityProcessor)]),
        format!("{:?}", config.engine).to_lowercase(),
        config.model_id,
    );
    lifecycle.ready();
    Ok((Arc::new(service), defaults))
}

async fn initialize_tts(
    config: TtsConfig,
    lifecycle: TtsLifecycle,
) -> Result<(Arc<TtsService>, Vec<String>, TtsCapabilities), SophonError> {
    let provider_model = match acquisition::resolve_tts_location(
        config.provider_id(),
        config.model_id(),
        config.operational.model_path.as_deref(),
    )? {
        TtsModelLocation::LocalOverride(path) => {
            if config.provider_id() == "qwentts-cpp" {
                TtsProviderModel::Qwen(acquisition::resolve_qwen_override(
                    &path,
                    config.provider_id(),
                    config.model_id(),
                )?)
            } else {
                TtsProviderModel::KokoroDirectory(path)
            }
        }
        TtsModelLocation::Registry(model) => {
            let model_dir = if let Some(path) =
                acquisition::validated_cache(&config.operational.cache_dir, model)
            {
                path
            } else if config.operational.automatic_download {
                let progress = lifecycle.clone();
                lifecycle.downloading(0.0);
                acquisition::acquire(&config.operational.cache_dir, model, move |value| {
                    progress.downloading(value)
                })
                .await?
            } else {
                return Err(SophonError::ModelUnavailable(
                    "TTS model is not cached and automatic downloads are disabled".into(),
                ));
            };
            if model.qwen.is_some() {
                TtsProviderModel::Qwen(acquisition::resolve_qwen_model(
                    &config.operational.cache_dir,
                    config.provider_id(),
                    config.model_id(),
                )?)
            } else {
                TtsProviderModel::KokoroDirectory(model_dir)
            }
        }
    };
    lifecycle.loading(config.provider_id(), config.model_id());
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
    let playback = PlaybackWorker::new(
        Box::new(PipeWirePlayback::default()),
        config.operational.queue_capacity,
    );
    let service = Arc::new(TtsService::new(lifecycle, worker, playback, config));
    Ok((service, voices, capabilities))
}

async fn run_async() -> Result<(), Box<dyn std::error::Error>> {
    install_qwen_log_bridge();
    let lifecycle = ModelLifecycle::new();
    // Claim the name with safe defaults; configuration and model work follows in
    // a background task so activation clients can observe readiness immediately.
    let connection = zbus::connection::Builder::session()?
        .name(BUS_NAME)?
        .serve_at(
            OBJECT_PATH,
            SophonDbus::unavailable(
                TranscriptionOptions {
                    language: Some("en".into()),
                    translate: Some(false),
                },
                lifecycle.clone(),
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
    let tts_lifecycle = TtsLifecycle::new();
    interface
        .get_mut()
        .await
        .set_tts_lifecycle(tts_lifecycle.clone());

    let config_interface = interface.clone();
    let config_stt_lifecycle = lifecycle.clone();
    let config_tts_lifecycle = tts_lifecycle.clone();
    tokio::spawn(async move {
        let config = ConfigPaths::discover()
            .map_err(|error| SophonError::ModelUnavailable(error.to_string()))
            .and_then(|paths| {
                Config::load(&paths)
                    .map_err(|error| SophonError::ModelUnavailable(error.to_string()))
            });
        let config = match config {
            Ok(config) => config,
            Err(error) => {
                config_stt_lifecycle.failed(error.to_string());
                config_tts_lifecycle.failed(error.to_string());
                return;
            }
        };

        let stt_config = config.clone();
        let stt_interface = config_interface.clone();
        let stt_lifecycle = config_stt_lifecycle.clone();
        tokio::spawn(async move {
            let max_audio_bytes = stt_config.max_audio_bytes;
            let max_audio_seconds = stt_config.max_audio_seconds;
            match initialize(stt_config, stt_lifecycle.clone()).await {
                Ok((service, defaults)) => {
                    stt_interface.get_mut().await.install(
                        defaults,
                        service,
                        max_audio_bytes,
                        max_audio_seconds,
                    );
                }
                Err(error) => stt_lifecycle.failed(error.to_string()),
            }
        });

        match config.tts {
            Err(error) => config_tts_lifecycle.failed(error),
            Ok(tts_config) => {
                let tts_interface = config_interface;
                let tts_lifecycle = config_tts_lifecycle;
                tokio::spawn(async move {
                    match initialize_tts(tts_config.clone(), tts_lifecycle.clone()).await {
                        Ok((service, voices, capabilities)) => {
                            tts_interface.get_mut().await.install_tts(
                                tts_config,
                                tts_lifecycle.clone(),
                                service,
                            );
                            tts_lifecycle.ready(voices, capabilities);
                        }
                        Err(error) => tts_lifecycle.failed(error.to_string()),
                    }
                });
            }
        }
    });

    let stt_observer_interface = interface.clone();
    tokio::spawn(async move {
        let mut previous = lifecycle.snapshot();
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            let current = lifecycle.snapshot();
            if current == previous {
                continue;
            }
            previous = current;
            let _ = SophonDbus::emit_lifecycle_changed(&stt_observer_interface).await;
        }
    });
    let tts_observer_interface = interface;
    tokio::spawn(async move {
        let mut previous = tts_lifecycle.snapshot();
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            let current = tts_lifecycle.snapshot();
            if current == previous {
                continue;
            }
            previous = current;
            let _ = SophonDbus::emit_tts_lifecycle_changed(&tts_observer_interface).await;
        }
    });
    tokio::signal::ctrl_c().await?;
    Ok(())
}
