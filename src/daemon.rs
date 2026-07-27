//! Daemon composition and lifecycle startup.

use std::sync::Arc;

use crate::{
    acquisition::{self, ModelLifecycle, ModelLocation},
    backend,
    config::{Config, ConfigPaths, DEFAULT_MAX_AUDIO_BYTES, DEFAULT_MAX_AUDIO_SECONDS},
    dbus::SophonDbus,
    domain::{SophonError, TranscriptionOptions},
    postprocess::{IdentityProcessor, PostProcessingPipeline},
    service::TranscriptionService,
    transport::{BUS_NAME, OBJECT_PATH},
    worker::ModelWorker,
};

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

async fn run_async() -> Result<(), Box<dyn std::error::Error>> {
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
    let init_interface = interface.clone();
    let init_lifecycle = lifecycle.clone();
    tokio::spawn(async move {
        let result = async {
            let paths = ConfigPaths::discover()
                .map_err(|error| SophonError::ModelUnavailable(error.to_string()))?;
            let config = Config::load(&paths)
                .map_err(|error| SophonError::ModelUnavailable(error.to_string()))?;
            let max_audio_bytes = config.max_audio_bytes;
            let max_audio_seconds = config.max_audio_seconds;
            let (service, defaults) = initialize(config, init_lifecycle.clone()).await?;
            let mut iface = init_interface.get_mut().await;
            iface.install(defaults, service, max_audio_bytes, max_audio_seconds);
            Ok::<(), SophonError>(())
        }
        .await;
        if let Err(error) = result {
            init_lifecycle.failed(error.to_string());
        }
    });
    tokio::spawn(async move {
        let mut previous = lifecycle.snapshot();
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            let current = lifecycle.snapshot();
            if current == previous {
                continue;
            }
            previous = current;
            let _ = SophonDbus::emit_lifecycle_changed(&interface).await;
        }
    });
    tokio::signal::ctrl_c().await?;
    Ok(())
}
