//! Transport-independent speech-to-text application service.

use crate::{
    error::SophonError,
    stt::{STTWorker, TranscriptionOptions, backend::to_transcribe_options},
};

pub struct STTService {
    worker: STTWorker,
    defaults: TranscriptionOptions,
    supported_languages: Vec<String>,
}

impl STTService {
    pub fn new(
        worker: STTWorker,
        defaults: TranscriptionOptions,
        supported_languages: Vec<String>,
    ) -> Self {
        Self {
            worker,
            defaults,
            supported_languages,
        }
    }

    pub async fn transcribe(
        &self,
        samples: Vec<f32>,
        options: TranscriptionOptions,
    ) -> Result<String, SophonError> {
        let options = to_transcribe_options(&options, &self.defaults, &self.supported_languages)?;
        self.worker.transcribe(samples, options).await
    }
}
