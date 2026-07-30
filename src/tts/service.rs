//! Transport-independent text-to-speech application service.

use std::{os::fd::OwnedFd, path::Path};

use crate::{
    audio::{OwnedAudio, encode_float_wav, publish_exclusive, sealed_memfd},
    config::TtsConfig,
    error::SophonError,
    tts::{
        TtsRequest, TtsWorker,
        playback::{PlaybackRequest, PlaybackWorker},
    },
};

pub struct TtsService {
    worker: TtsWorker,
    playback: PlaybackWorker,
    config: TtsConfig,
    supported_languages: Vec<String>,
    aloud: tokio::sync::Mutex<()>,
}

impl TtsService {
    pub fn new(
        worker: TtsWorker,
        playback: PlaybackWorker,
        config: TtsConfig,
        supported_languages: Vec<String>,
    ) -> Self {
        Self {
            worker,
            playback,
            config,
            supported_languages,
            aloud: tokio::sync::Mutex::new(()),
        }
    }

    fn validate_language(&self, request: &TtsRequest) -> Result<(), SophonError> {
        let Some(language) = request.language.as_deref() else {
            return Ok(());
        };
        let normalized = language.to_ascii_lowercase();
        let base = match normalized.as_str() {
            "cmn" => "zh",
            value => value.split('-').next().unwrap_or(value),
        };
        if self
            .supported_languages
            .iter()
            .any(|supported| supported == base)
        {
            Ok(())
        } else {
            Err(SophonError::InvalidTtsOptions(format!(
                "language `{language}` is unsupported by the active model"
            )))
        }
    }

    async fn synthesize(&self, request: TtsRequest) -> Result<OwnedAudio, SophonError> {
        self.validate_language(&request)?;
        self.worker.synthesize(request).await
    }

    pub async fn speak_to_file(
        &self,
        request: TtsRequest,
        path: &Path,
    ) -> Result<u64, SophonError> {
        if !path.is_absolute() {
            return Err(SophonError::OutputFailed(
                "output path must be absolute".into(),
            ));
        }
        if path.exists() {
            return Err(SophonError::OutputExists(path.display().to_string()));
        }
        self.validate_language(&request)?;
        let audio = self.worker.synthesize(request).await?;
        let wav = encode_float_wav(&audio, self.config.operational.max_generated_audio_seconds)?;
        publish_exclusive(path, &wav)
    }

    pub async fn speak_to_buffer(
        &self,
        request: TtsRequest,
    ) -> Result<(OwnedFd, u64), SophonError> {
        let audio = self.synthesize(request).await?;
        let wav = encode_float_wav(&audio, self.config.operational.max_generated_audio_seconds)?;
        sealed_memfd(&wav)
    }

    pub async fn speak_aloud(&self, request: TtsRequest) -> Result<(), SophonError> {
        self.validate_language(&request)?;
        // Tokio's mutex is FIFO, so the serialized stage covers both provider
        // generation and playback without blocking an async executor thread.
        let _aloud = self.aloud.lock().await;
        let stream = self.worker.synthesize_streaming(request)?;
        self.playback
            .play(PlaybackRequest {
                stream,
                output_device: self.config.operational.output_device.clone(),
                volume: self.config.operational.volume as f32,
            })
            .await
    }
}
