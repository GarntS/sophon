//! Session-bus object exported by the daemon.

use std::{collections::HashMap, os::fd::OwnedFd, sync::Arc};

use zbus::{DBusError, zvariant::OwnedValue};

use crate::{
    acquisition::ModelLifecycle,
    audio::{read_file, read_unix_fd},
    domain::{ModelState, SophonError, TranscriptionOptions},
    service::TranscriptionService,
    transport::{OptionValue, decode_options},
};

#[derive(Debug, DBusError)]
#[zbus(prefix = "com.garntresearch.sophon")]
pub enum SophonDbusError {
    NotReady(String),
    InvalidOptions(String),
    InvalidAudio(String),
    ModelUnavailable(String),
    ResourceLimit(String),
    TranscriptionFailed(String),
}

impl From<SophonError> for SophonDbusError {
    fn from(error: SophonError) -> Self {
        match error {
            SophonError::NotReady => Self::NotReady("model is not ready".into()),
            SophonError::InvalidOptions(message) => Self::InvalidOptions(message),
            SophonError::InvalidAudio(message) => Self::InvalidAudio(message),
            SophonError::ModelUnavailable(message) => Self::ModelUnavailable(message),
            SophonError::ResourceLimit(message) => Self::ResourceLimit(message),
            SophonError::TranscriptionFailed(message) => Self::TranscriptionFailed(message),
        }
    }
}

pub struct SophonDbus {
    defaults: TranscriptionOptions,
    lifecycle: ModelLifecycle,
    service: Option<Arc<TranscriptionService>>,
    max_audio_bytes: u64,
    max_audio_seconds: u64,
}

impl SophonDbus {
    pub fn unavailable(
        defaults: TranscriptionOptions,
        lifecycle: ModelLifecycle,
        max_audio_bytes: u64,
        max_audio_seconds: u64,
    ) -> Self {
        Self {
            defaults,
            lifecycle,
            service: None,
            max_audio_bytes,
            max_audio_seconds,
        }
    }

    pub fn ready(
        defaults: TranscriptionOptions,
        lifecycle: ModelLifecycle,
        service: Arc<TranscriptionService>,
        max_audio_bytes: u64,
        max_audio_seconds: u64,
    ) -> Self {
        Self {
            defaults,
            lifecycle,
            service: Some(service),
            max_audio_bytes,
            max_audio_seconds,
        }
    }

    fn options(
        &self,
        values: HashMap<String, OwnedValue>,
    ) -> Result<TranscriptionOptions, SophonDbusError> {
        let mut decoded = std::collections::BTreeMap::new();
        for (key, value) in values {
            if let Ok(value) = String::try_from(value.clone()) {
                decoded.insert(key, OptionValue::String(value));
            } else if let Ok(value) = bool::try_from(value) {
                decoded.insert(key, OptionValue::Bool(value));
            } else {
                return Err(SophonDbusError::InvalidOptions(
                    "unsupported option type".into(),
                ));
            }
        }
        decode_options(decoded, &self.defaults).map_err(Into::into)
    }

    pub fn install(
        &mut self,
        defaults: TranscriptionOptions,
        service: Arc<TranscriptionService>,
        max_audio_bytes: u64,
        max_audio_seconds: u64,
    ) {
        self.defaults = defaults;
        self.service = Some(service);
        self.max_audio_bytes = max_audio_bytes;
        self.max_audio_seconds = max_audio_seconds;
    }

    fn service(&self) -> Result<&Arc<TranscriptionService>, SophonDbusError> {
        self.service
            .as_ref()
            .ok_or_else(|| match self.lifecycle.snapshot().state {
                ModelState::Failed { message } => SophonDbusError::ModelUnavailable(message),
                _ => SophonDbusError::NotReady("model is not ready".into()),
            })
    }

    /// Emits standard `PropertiesChanged` notifications for every observable
    /// lifecycle property. The daemon calls this after a snapshot change, and
    /// the integration harness uses the same path.
    pub async fn emit_lifecycle_changed(
        interface: &zbus::object_server::InterfaceRef<Self>,
    ) -> zbus::Result<()> {
        let emitter = interface.signal_emitter();
        let iface = interface.get().await;
        iface.state_changed(emitter).await?;
        iface.active_engine_changed(emitter).await?;
        iface.active_model_changed(emitter).await?;
        iface.download_progress_changed(emitter).await?;
        iface.last_error_changed(emitter).await?;
        Ok(())
    }
}

#[zbus::interface(name = "com.garntresearch.sophon")]
impl SophonDbus {
    #[zbus(name = "TranscribeFile")]
    async fn transcribe_file(
        &self,
        path: String,
        values: HashMap<String, OwnedValue>,
    ) -> Result<String, SophonDbusError> {
        let options = self.options(values)?;
        let samples = read_file(path.as_ref(), self.max_audio_bytes, self.max_audio_seconds)?;
        self.service()?
            .transcribe(samples, options)
            .await
            .map_err(Into::into)
    }

    #[zbus(name = "TranscribeMemfd")]
    async fn transcribe_memfd(
        &self,
        fd: zbus::zvariant::OwnedFd,
        values: HashMap<String, OwnedValue>,
    ) -> Result<String, SophonDbusError> {
        let options = self.options(values)?;
        let fd: OwnedFd = fd.into();
        let samples = read_unix_fd(fd, self.max_audio_bytes, self.max_audio_seconds)?;
        self.service()?
            .transcribe(samples, options)
            .await
            .map_err(Into::into)
    }

    #[zbus(property)]
    fn state(&self) -> String {
        match self.lifecycle.snapshot().state {
            ModelState::Initializing => "Initializing".into(),
            ModelState::Downloading { .. } => "Downloading".into(),
            ModelState::Loading => "Loading".into(),
            ModelState::Ready => "Ready".into(),
            ModelState::Failed { .. } => "Failed".into(),
        }
    }

    #[zbus(property)]
    fn active_engine(&self) -> String {
        self.lifecycle
            .snapshot()
            .active_engine
            .map(|engine| format!("{engine:?}").to_lowercase())
            .unwrap_or_default()
    }
    #[zbus(property)]
    fn active_model(&self) -> String {
        self.lifecycle.snapshot().active_model.unwrap_or_default()
    }
    #[zbus(property)]
    fn download_progress(&self) -> f64 {
        self.lifecycle.snapshot().download_progress as f64
    }
    #[zbus(property)]
    fn last_error(&self) -> String {
        self.lifecycle.snapshot().last_error.unwrap_or_default()
    }
}
