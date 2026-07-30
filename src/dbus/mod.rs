//! Session-bus object exported by the daemon.

pub mod transport;

use std::{collections::HashMap, os::fd::OwnedFd, sync::Arc};

use zbus::{DBusError, zvariant::OwnedValue};

use crate::{
    audio::{read_file, read_unix_fd},
    config::TtsConfig,
    error::SophonError,
    provider_runtime::{ModelState, SttProviderHandle, TtsProviderHandle},
    stt::{STTService, TranscriptionOptions},
    tts::{TtsRequest, TtsService},
};

use self::transport::{OptionValue, TtsOptionValue, decode_options, decode_tts_options};

fn decode_dbus_tts_values(
    values: HashMap<String, OwnedValue>,
) -> Result<std::collections::BTreeMap<String, TtsOptionValue>, SophonDbusError> {
    let mut decoded = std::collections::BTreeMap::new();
    for (key, value) in values {
        let option = match key.as_str() {
            "voice" | "language" | "clone_transcript" | "voice_description" => {
                TtsOptionValue::String(String::try_from(value).map_err(|_| {
                    SophonDbusError::InvalidTtsOptions(format!("option `{key}` must be a string"))
                })?)
            }
            "speed" => TtsOptionValue::Double(f64::try_from(value).map_err(|_| {
                SophonDbusError::InvalidTtsOptions("option `speed` must be a double".into())
            })?),
            "clone_audio" => {
                let fd = zbus::zvariant::Fd::try_from(value).map_err(|_| {
                    SophonDbusError::InvalidTtsOptions(
                        "option `clone_audio` must be a transferred Unix descriptor".into(),
                    )
                })?;
                let fd = OwnedFd::try_from(fd).map_err(|_| {
                    SophonDbusError::InvalidTtsOptions(
                        "option `clone_audio` descriptor could not be owned".into(),
                    )
                })?;
                TtsOptionValue::UnixFd(fd)
            }
            _ => {
                return Err(SophonDbusError::InvalidTtsOptions(format!(
                    "unknown option `{key}`"
                )));
            }
        };
        decoded.insert(key, option);
    }
    Ok(decoded)
}

#[derive(Debug, DBusError)]
#[zbus(prefix = "com.garntresearch.sophon")]
pub enum SophonDbusError {
    NotReady(String),
    InvalidOptions(String),
    InvalidAudio(String),
    ModelUnavailable(String),
    ResourceLimit(String),
    TranscriptionFailed(String),
    InvalidTtsOptions(String),
    InvalidReferenceAudio(String),
    UnsupportedCapability(String),
    OutputExists(String),
    OutputFailed(String),
    SynthesisFailed(String),
    PlaybackFailed(String),
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
            SophonError::InvalidTtsOptions(message) => Self::InvalidTtsOptions(message),
            SophonError::InvalidReferenceAudio(message) => Self::InvalidReferenceAudio(message),
            SophonError::UnsupportedCapability(message) => Self::UnsupportedCapability(message),
            SophonError::OutputExists(message) => Self::OutputExists(message),
            SophonError::OutputFailed(message) => Self::OutputFailed(message),
            SophonError::SynthesisFailed(message) => Self::SynthesisFailed(message),
            SophonError::PlaybackFailed(message) => Self::PlaybackFailed(message),
        }
    }
}

pub struct SophonDbus {
    defaults: TranscriptionOptions,
    stt_handle: SttProviderHandle,
    max_audio_bytes: u64,
    max_audio_seconds: u64,
    tts_config: Option<TtsConfig>,
    tts_handle: TtsProviderHandle,
}

impl SophonDbus {
    pub fn unavailable(
        defaults: TranscriptionOptions,
        stt_handle: SttProviderHandle,
        tts_handle: TtsProviderHandle,
        max_audio_bytes: u64,
        max_audio_seconds: u64,
    ) -> Self {
        Self {
            defaults,
            stt_handle,
            max_audio_bytes,
            max_audio_seconds,
            tts_config: None,
            tts_handle,
        }
    }

    pub fn ready(
        defaults: TranscriptionOptions,
        stt_handle: SttProviderHandle,
        tts_handle: TtsProviderHandle,
        max_audio_bytes: u64,
        max_audio_seconds: u64,
    ) -> Self {
        Self::unavailable(
            defaults,
            stt_handle,
            tts_handle,
            max_audio_bytes,
            max_audio_seconds,
        )
    }

    fn options(
        &self,
        values: HashMap<String, OwnedValue>,
    ) -> Result<TranscriptionOptions, SophonDbusError> {
        let mut decoded = std::collections::BTreeMap::new();
        for (key, value) in values {
            let value = String::try_from(value).map_err(|_| {
                SophonDbusError::InvalidOptions(format!("option `{key}` must be a string"))
            })?;
            decoded.insert(key, OptionValue::String(value));
        }
        decode_options(decoded, &self.defaults).map_err(Into::into)
    }

    pub fn install(
        &mut self,
        defaults: TranscriptionOptions,
        max_audio_bytes: u64,
        max_audio_seconds: u64,
    ) {
        self.defaults = defaults;
        self.max_audio_bytes = max_audio_bytes;
        self.max_audio_seconds = max_audio_seconds;
    }

    pub fn install_tts(&mut self, config: TtsConfig) {
        self.tts_config = Some(config);
    }

    fn service(&self) -> Result<Arc<STTService>, SophonDbusError> {
        self.stt_handle
            .service()
            .ok_or_else(|| match self.stt_handle.state() {
                ModelState::Failed { message } => SophonDbusError::ModelUnavailable(message),
                _ => SophonDbusError::NotReady("model is not ready".into()),
            })
    }

    fn tts_service(&self) -> Result<Arc<TtsService>, SophonDbusError> {
        self.tts_handle
            .service()
            .ok_or_else(|| match self.tts_handle.state() {
                ModelState::Failed { message } => SophonDbusError::ModelUnavailable(message),
                _ => SophonDbusError::NotReady("TTS model is not ready".into()),
            })
    }

    fn tts_request(
        &self,
        text: &str,
        values: HashMap<String, OwnedValue>,
    ) -> Result<TtsRequest, SophonDbusError> {
        let config = self
            .tts_config
            .as_ref()
            .ok_or_else(|| SophonDbusError::NotReady("TTS configuration is not ready".into()))?;
        let snapshot = self.tts_handle.snapshot();
        decode_tts_options(
            text,
            decode_dbus_tts_values(values)?,
            config,
            snapshot.capabilities,
            &snapshot.available_voices,
        )
        .map_err(Into::into)
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
        iface.active_provider_changed(emitter).await?;
        iface.active_model_changed(emitter).await?;
        iface.download_progress_changed(emitter).await?;
        iface.last_error_changed(emitter).await?;
        Ok(())
    }

    pub async fn emit_tts_lifecycle_changed(
        interface: &zbus::object_server::InterfaceRef<Self>,
    ) -> zbus::Result<()> {
        let emitter = interface.signal_emitter();
        let iface = interface.get().await;
        iface.tts_state_changed(emitter).await?;
        iface.active_tts_provider_changed(emitter).await?;
        iface.active_tts_model_changed(emitter).await?;
        iface.tts_download_progress_changed(emitter).await?;
        iface.tts_last_error_changed(emitter).await?;
        iface.available_voices_changed(emitter).await?;
        iface.tts_capabilities_changed(emitter).await?;
        Ok(())
    }
}

fn state_name(state: ModelState) -> String {
    match state {
        ModelState::Initializing => "Initializing".into(),
        ModelState::Downloading { .. } => "Downloading".into(),
        ModelState::Loading => "Loading".into(),
        ModelState::Ready => "Ready".into(),
        ModelState::Failed { .. } => "Failed".into(),
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
        let audio = read_file(path.as_ref(), self.max_audio_bytes, self.max_audio_seconds)?;
        self.service()?
            .transcribe(audio, options)
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
        let audio = read_unix_fd(fd, self.max_audio_bytes, self.max_audio_seconds)?;
        self.service()?
            .transcribe(audio, options)
            .await
            .map_err(Into::into)
    }

    #[zbus(name = "SpeakToFile")]
    async fn speak_to_file(
        &self,
        text: String,
        path: String,
        values: HashMap<String, OwnedValue>,
    ) -> Result<u64, SophonDbusError> {
        let service = self.tts_service()?;
        let request = self.tts_request(&text, values)?;
        service
            .speak_to_file(request, path.as_ref())
            .await
            .map_err(Into::into)
    }

    #[zbus(name = "SpeakToBuffer")]
    async fn speak_to_buffer(
        &self,
        text: String,
        values: HashMap<String, OwnedValue>,
    ) -> Result<(zbus::zvariant::OwnedFd, u64), SophonDbusError> {
        let service = self.tts_service()?;
        let request = self.tts_request(&text, values)?;
        let (fd, length) = service.speak_to_buffer(request).await?;
        Ok((fd.into(), length))
    }

    #[zbus(name = "SpeakAloud")]
    async fn speak_aloud(
        &self,
        text: String,
        values: HashMap<String, OwnedValue>,
    ) -> Result<(), SophonDbusError> {
        let service = self.tts_service()?;
        let request = self.tts_request(&text, values)?;
        service.speak_aloud(request).await.map_err(Into::into)
    }

    #[zbus(property)]
    fn tts_state(&self) -> String {
        state_name(self.tts_handle.state())
    }

    #[zbus(property)]
    fn active_tts_provider(&self) -> String {
        self.tts_handle.snapshot().provider.active_provider
    }

    #[zbus(property)]
    fn active_tts_model(&self) -> String {
        self.tts_handle.snapshot().provider.active_model
    }

    #[zbus(property)]
    fn tts_download_progress(&self) -> f64 {
        self.tts_handle.snapshot().provider.download_progress as f64
    }

    #[zbus(property)]
    fn tts_last_error(&self) -> String {
        self.tts_handle
            .snapshot()
            .provider
            .last_error
            .unwrap_or_default()
    }

    #[zbus(property)]
    fn available_voices(&self) -> Vec<String> {
        self.tts_handle.snapshot().available_voices
    }

    #[zbus(property)]
    fn tts_capabilities(&self) -> Vec<String> {
        let capabilities = self.tts_handle.snapshot().capabilities;
        let mut values = Vec::new();
        if capabilities.named_voices {
            values.push("named-voices".into());
        }
        if capabilities.voice_cloning {
            values.push("voice-cloning".into());
        }
        if capabilities.voice_design {
            values.push("voice-design".into());
        }
        if capabilities.speed_control {
            values.push("speed-control".into());
        }
        values
    }

    #[zbus(property)]
    fn state(&self) -> String {
        state_name(self.stt_handle.state())
    }

    #[zbus(property)]
    fn active_provider(&self) -> String {
        self.stt_handle.snapshot().active_provider
    }
    #[zbus(property)]
    fn active_model(&self) -> String {
        self.stt_handle.snapshot().active_model
    }
    #[zbus(property)]
    fn download_progress(&self) -> f64 {
        self.stt_handle.snapshot().download_progress as f64
    }
    #[zbus(property)]
    fn last_error(&self) -> String {
        self.stt_handle.snapshot().last_error.unwrap_or_default()
    }
}
