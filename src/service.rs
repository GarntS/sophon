//! Transport-independent transcription application service.

use std::sync::Arc;

use crate::{
    acquisition::ModelLifecycle,
    backend::to_transcribe_options,
    domain::{ModelState, SophonError, Transcript, TranscriptionOptions},
    postprocess::PostProcessingPipeline,
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
