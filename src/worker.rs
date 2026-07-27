//! Bounded serialized scheduling for mutable synchronous model inference.

use std::{
    sync::mpsc::{SyncSender, TrySendError, sync_channel},
    thread,
};

use tokio::sync::oneshot;
use transcribe_rs::{SpeechModel, TranscribeOptions};

use crate::domain::SophonError;

struct WorkItem {
    samples: Vec<f32>,
    options: TranscribeOptions,
    response: oneshot::Sender<Result<String, SophonError>>,
}

#[derive(Clone)]
pub struct ModelWorker {
    sender: SyncSender<WorkItem>,
}

impl ModelWorker {
    pub fn new(mut model: Box<dyn SpeechModel>, capacity: usize) -> Self {
        let (sender, receiver) = sync_channel::<WorkItem>(capacity);
        thread::Builder::new()
            .name("sophon-model-worker".into())
            .spawn(move || {
                while let Ok(work) = receiver.recv() {
                    let result = model
                        .transcribe(&work.samples, &work.options)
                        .map(|result| result.text)
                        .map_err(|error| SophonError::TranscriptionFailed(error.to_string()));
                    let _ = work.response.send(result);
                }
            })
            .expect("failed to create model worker");
        Self { sender }
    }

    pub async fn transcribe(
        &self,
        samples: Vec<f32>,
        options: TranscribeOptions,
    ) -> Result<String, SophonError> {
        let (response, receiver) = oneshot::channel();
        match self.sender.try_send(WorkItem {
            samples,
            options,
            response,
        }) {
            Ok(()) => receiver
                .await
                .map_err(|_| SophonError::TranscriptionFailed("model worker stopped".into()))?,
            Err(TrySendError::Full(_)) => Err(SophonError::ResourceLimit(
                "transcription queue is full".into(),
            )),
            Err(TrySendError::Disconnected(_)) => Err(SophonError::ModelUnavailable(
                "model worker is unavailable".into(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::{Arc, Mutex},
        time::Duration,
    };
    use transcribe_rs::{ModelCapabilities, TranscribeError, TranscriptionResult};

    struct FixtureModel {
        calls: Arc<Mutex<Vec<f32>>>,
        fail_first: bool,
    }

    impl SpeechModel for FixtureModel {
        fn capabilities(&self) -> ModelCapabilities {
            ModelCapabilities {
                name: "fixture",
                engine_id: "fixture",
                sample_rate: 16_000,
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
            self.calls.lock().unwrap().push(samples[0]);
            if self.fail_first {
                self.fail_first = false;
                return Err(TranscribeError::Inference("fixture failure".into()));
            }
            std::thread::sleep(Duration::from_millis(20));
            Ok(TranscriptionResult {
                text: samples[0].to_string(),
                segments: None,
            })
        }
    }

    #[tokio::test]
    async fn serializes_work_and_continues_after_an_inference_failure() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let worker = ModelWorker::new(
            Box::new(FixtureModel {
                calls: calls.clone(),
                fail_first: true,
            }),
            2,
        );
        assert!(matches!(
            worker
                .transcribe(vec![1.0], TranscribeOptions::default())
                .await,
            Err(SophonError::TranscriptionFailed(_))
        ));
        let (second, third) = tokio::join!(
            worker.transcribe(vec![2.0], TranscribeOptions::default()),
            worker.transcribe(vec![3.0], TranscribeOptions::default()),
        );
        assert_eq!(second.unwrap(), "2");
        assert_eq!(third.unwrap(), "3");
        assert_eq!(*calls.lock().unwrap(), vec![1.0, 2.0, 3.0]);
    }

    #[tokio::test]
    async fn rejects_work_when_the_bounded_queue_is_full() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let worker = ModelWorker::new(
            Box::new(FixtureModel {
                calls,
                fail_first: false,
            }),
            1,
        );
        let first = worker.transcribe(vec![1.0], TranscribeOptions::default());
        tokio::pin!(first);
        tokio::select! { _ = &mut first => panic!("fixture inference should not be instant"), _ = tokio::time::sleep(Duration::from_millis(1)) => {} }
        let queued = worker.transcribe(vec![2.0], TranscribeOptions::default());
        tokio::pin!(queued);
        tokio::select! { _ = &mut queued => panic!("queued work should wait"), _ = tokio::time::sleep(Duration::from_millis(1)) => {} }
        assert!(matches!(
            worker
                .transcribe(vec![3.0], TranscribeOptions::default())
                .await,
            Err(SophonError::ResourceLimit(_))
        ));
        assert!(first.await.is_ok());
        assert!(queued.await.is_ok());
    }
}
