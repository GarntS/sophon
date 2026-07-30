//! Transport-independent speech-to-text application service.

use crate::{
    audio::{DecodedWav, downmix_wav, resample_mono},
    error::SophonError,
    stt::{STTWorker, TranscriptionOptions, backend::to_transcribe_options},
};

pub struct STTService {
    worker: STTWorker,
    defaults: TranscriptionOptions,
    supported_languages: Vec<String>,
    model_sample_rate: u32,
    max_audio_seconds: u64,
}

impl STTService {
    pub fn new(
        worker: STTWorker,
        defaults: TranscriptionOptions,
        supported_languages: Vec<String>,
        model_sample_rate: u32,
        max_audio_seconds: u64,
    ) -> Self {
        Self {
            worker,
            defaults,
            supported_languages,
            model_sample_rate,
            max_audio_seconds,
        }
    }

    pub async fn transcribe(
        &self,
        audio: DecodedWav,
        options: TranscriptionOptions,
    ) -> Result<String, SophonError> {
        if self.model_sample_rate == 0 {
            return Err(SophonError::ModelUnavailable(
                "active STT model advertises a zero sample rate".into(),
            ));
        }
        let audio = resample_mono(downmix_wav(audio)?, self.model_sample_rate)?;
        if audio.samples.len() as u128
            > u128::from(self.model_sample_rate) * u128::from(self.max_audio_seconds)
        {
            return Err(SophonError::ResourceLimit(
                "normalized audio exceeds duration limit".into(),
            ));
        }
        let options = to_transcribe_options(&options, &self.defaults, &self.supported_languages)?;
        self.worker.transcribe(audio.samples, options).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use transcribe_rs::{
        ModelCapabilities, SpeechModel, TranscribeError, TranscribeOptions, TranscriptionResult,
    };

    struct FixtureModel;

    impl SpeechModel for FixtureModel {
        fn capabilities(&self) -> ModelCapabilities {
            ModelCapabilities {
                name: "fixture",
                engine_id: "fixture",
                sample_rate: 4,
                languages: &["en"],
                supports_timestamps: false,
                supports_translation: false,
                supports_streaming: false,
            }
        }

        fn transcribe_raw(
            &mut self,
            samples: &[f32],
            _: &TranscribeOptions,
        ) -> Result<TranscriptionResult, TranscribeError> {
            Ok(TranscriptionResult {
                text: samples.len().to_string(),
                segments: None,
            })
        }
    }

    #[tokio::test]
    async fn enforces_normalized_model_input_duration() {
        let service = STTService::new(
            STTWorker::new(Box::new(FixtureModel), 1),
            TranscriptionOptions::default(),
            vec!["en".into()],
            4,
            1,
        );
        let result = service
            .transcribe(
                DecodedWav {
                    samples: vec![0.0; 5],
                    source_rate: 4,
                    channels: 1,
                },
                TranscriptionOptions::default(),
            )
            .await;
        assert!(matches!(result, Err(SophonError::ResourceLimit(_))));
    }
}
