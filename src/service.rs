//! Transport-independent transcription application service.

use std::{os::fd::OwnedFd, path::Path, sync::Arc};

use crate::{
    acquisition::{ModelLifecycle, TtsLifecycle},
    audio::{encode_float_wav, publish_exclusive, sealed_memfd},
    backend::to_transcribe_options,
    config::TtsConfig,
    domain::{ModelState, SophonError, Transcript, TranscriptionOptions, TtsRequest, TtsState},
    playback::{PlaybackRequest, PlaybackWorker},
    postprocess::PostProcessingPipeline,
    tts::TtsWorker,
    worker::ModelWorker,
};

pub struct TranscriptionService {
    lifecycle: ModelLifecycle,
    worker: ModelWorker,
    defaults: TranscriptionOptions,
    capabilities: crate::acquisition::ModelCapabilities,
    postprocessors: Arc<PostProcessingPipeline>,
    engine: String,
    model: String,
}

impl TranscriptionService {
    pub fn new(
        lifecycle: ModelLifecycle,
        worker: ModelWorker,
        defaults: TranscriptionOptions,
        capabilities: crate::acquisition::ModelCapabilities,
        postprocessors: PostProcessingPipeline,
        engine: String,
        model: String,
    ) -> Self {
        Self {
            lifecycle,
            worker,
            defaults,
            capabilities,
            postprocessors: Arc::new(postprocessors),
            engine,
            model,
        }
    }

    pub async fn transcribe(
        &self,
        samples: Vec<f32>,
        options: TranscriptionOptions,
    ) -> Result<String, SophonError> {
        match self.lifecycle.snapshot().state {
            ModelState::Ready => {}
            ModelState::Failed { .. } => {
                return Err(SophonError::ModelUnavailable(
                    "model initialization failed".into(),
                ));
            }
            _ => return Err(SophonError::NotReady),
        }
        let options = to_transcribe_options(&options, &self.defaults, &self.capabilities)?;
        let raw_text = self.worker.transcribe(samples, options).await?;
        let transcript = self.postprocessors.process(Transcript {
            raw_text: raw_text.clone(),
            final_text: raw_text,
            segments: vec![],
            engine: self.engine.clone(),
            model: self.model.clone(),
        });
        Ok(transcript.final_text)
    }
}

pub struct TtsService {
    lifecycle: TtsLifecycle,
    worker: TtsWorker,
    playback: PlaybackWorker,
    config: TtsConfig,
}

impl TtsService {
    pub fn new(
        lifecycle: TtsLifecycle,
        worker: TtsWorker,
        playback: PlaybackWorker,
        config: TtsConfig,
    ) -> Self {
        Self {
            lifecycle,
            worker,
            playback,
            config,
        }
    }

    fn ensure_ready(&self) -> Result<(), SophonError> {
        match self.lifecycle.snapshot().state {
            TtsState::Ready => Ok(()),
            TtsState::Failed { .. } => Err(SophonError::ModelUnavailable(
                "TTS model initialization failed".into(),
            )),
            _ => Err(SophonError::NotReady),
        }
    }

    async fn synthesize(
        &self,
        request: TtsRequest,
    ) -> Result<crate::domain::OwnedAudio, SophonError> {
        self.ensure_ready()?;
        self.worker.synthesize(request).await
    }

    pub async fn speak_to_file(
        &self,
        request: TtsRequest,
        path: &Path,
    ) -> Result<u64, SophonError> {
        self.ensure_ready()?;
        if !path.is_absolute() {
            return Err(SophonError::OutputFailed(
                "output path must be absolute".into(),
            ));
        }
        if path.exists() {
            return Err(SophonError::OutputExists(path.display().to_string()));
        }
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
        let audio = self.synthesize(request).await?;
        self.playback
            .play(PlaybackRequest {
                audio,
                node_name: self.config.operational.pipewire_node.clone(),
                volume: self.config.operational.volume as f32,
            })
            .await
    }
}
