//! Bounded serialized scheduling and stream protocol for TTS inference.

use std::{
    sync::mpsc::{SyncSender, TrySendError, sync_channel},
    thread,
};

use tokio::sync::oneshot;

use crate::{
    audio::OwnedAudio,
    error::SophonError,
    tts::{TtsCapabilities, TtsProvider, TtsRequest, TtsStreamControl, TtsStreamEvent},
};

const STREAM_CHUNK_SAMPLES: usize = 4_096;
const STREAM_CHANNEL_EVENTS: usize = 4;

enum TtsWorkItem {
    Buffered {
        request: TtsRequest,
        response: oneshot::Sender<Result<OwnedAudio, SophonError>>,
    },
    Streaming {
        request: TtsRequest,
        events: tokio::sync::mpsc::Sender<TtsStreamEvent>,
        terminal: oneshot::Sender<Result<(), SophonError>>,
    },
}

pub struct TtsStream {
    events: tokio::sync::mpsc::Receiver<TtsStreamEvent>,
    terminal_receiver: Option<oneshot::Receiver<Result<(), SophonError>>>,
    terminal: Option<Result<(), SophonError>>,
    terminal_emitted: bool,
}

impl TtsStream {
    fn terminal_cancelled() -> SophonError {
        SophonError::SynthesisFailed("streaming synthesis stopped without a terminal event".into())
    }

    async fn next_terminal(&mut self) -> Option<TtsStreamEvent> {
        if self.terminal_emitted {
            return None;
        }
        let result = match self.terminal.take() {
            Some(result) => result,
            None => self
                .terminal_receiver
                .take()
                .expect("terminal receiver is present until emitted")
                .await
                .unwrap_or_else(|_| Err(Self::terminal_cancelled())),
        };
        self.terminal_emitted = true;
        Some(TtsStreamEvent::Terminal(result))
    }

    pub async fn next(&mut self) -> Option<TtsStreamEvent> {
        loop {
            match self.events.try_recv() {
                Ok(event) => return Some(event),
                Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                    return self.next_terminal().await;
                }
                Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {}
            }
            if self.terminal.is_some() {
                return self.next_terminal().await;
            }
            tokio::select! {
                event = self.events.recv() => {
                    if let Some(event) = event {
                        return Some(event);
                    }
                    return self.next_terminal().await;
                }
                result = self.terminal_receiver.as_mut().expect("terminal receiver is present until emitted") => {
                    self.terminal = Some(result.unwrap_or_else(|_| Err(Self::terminal_cancelled())));
                }
            }
        }
    }

    pub fn blocking_next(&mut self) -> Option<TtsStreamEvent> {
        if let Some(event) = self.events.blocking_recv() {
            return Some(event);
        }
        if self.terminal_emitted {
            return None;
        }
        let result = match self.terminal.take() {
            Some(result) => result,
            None => self
                .terminal_receiver
                .take()
                .expect("terminal receiver is present until emitted")
                .blocking_recv()
                .unwrap_or_else(|_| Err(Self::terminal_cancelled())),
        };
        self.terminal_emitted = true;
        Some(TtsStreamEvent::Terminal(result))
    }

    pub(crate) fn try_next(
        &mut self,
    ) -> Result<TtsStreamEvent, tokio::sync::mpsc::error::TryRecvError> {
        match self.events.try_recv() {
            Ok(event) => Ok(event),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {
                let _ = self.try_terminal();
                Err(tokio::sync::mpsc::error::TryRecvError::Empty)
            }
            Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                if self.terminal_emitted {
                    return Err(tokio::sync::mpsc::error::TryRecvError::Disconnected);
                }
                if self.terminal.is_none() {
                    match self.try_terminal() {
                        Ok(Some(_)) => {}
                        Ok(None) => return Err(tokio::sync::mpsc::error::TryRecvError::Empty),
                        Err(_) => {
                            return Err(tokio::sync::mpsc::error::TryRecvError::Disconnected);
                        }
                    }
                }
                let result = self
                    .terminal
                    .take()
                    .expect("received terminal result is retained");
                self.terminal_emitted = true;
                Ok(TtsStreamEvent::Terminal(result))
            }
        }
    }

    /// Allows playback to observe a terminal failure without waiting for queued audio.
    pub(crate) fn try_terminal(&mut self) -> Result<Option<Result<(), SophonError>>, SophonError> {
        if self.terminal.is_none() && !self.terminal_emitted {
            match self
                .terminal_receiver
                .as_mut()
                .expect("terminal receiver is present until emitted")
                .try_recv()
            {
                Ok(result) => self.terminal = Some(result),
                Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {}
                Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
                    return Err(Self::terminal_cancelled());
                }
            }
        }
        Ok(self.terminal.clone())
    }

    #[cfg(test)]
    pub(crate) fn from_events(events: impl IntoIterator<Item = TtsStreamEvent>) -> Self {
        let events: Vec<_> = events.into_iter().collect();
        let (sender, receiver) = tokio::sync::mpsc::channel(events.len().max(1));
        let (terminal_sender, terminal_receiver) = oneshot::channel();
        let mut terminal_sender = Some(terminal_sender);
        for event in events {
            match event {
                TtsStreamEvent::Terminal(result) => {
                    terminal_sender.take().unwrap().send(result).unwrap()
                }
                event => sender.try_send(event).unwrap(),
            }
        }
        drop(sender);
        Self {
            events: receiver,
            terminal_receiver: Some(terminal_receiver),
            terminal: None,
            terminal_emitted: false,
        }
    }
}

struct StreamValidator {
    sample_rate: Option<u32>,
    accepted_samples: u128,
    max_generated_audio_seconds: u64,
}

impl StreamValidator {
    fn new(max_generated_audio_seconds: u64) -> Self {
        Self {
            sample_rate: None,
            accepted_samples: 0,
            max_generated_audio_seconds,
        }
    }

    fn accept(&mut self, event: &TtsStreamEvent) -> Result<(), SophonError> {
        match event {
            TtsStreamEvent::Format { sample_rate } => {
                if *sample_rate == 0 || self.sample_rate.replace(*sample_rate).is_some() {
                    return Err(SophonError::SynthesisFailed(
                        "provider emitted an invalid stream format".into(),
                    ));
                }
            }
            TtsStreamEvent::Chunk { samples } => {
                let rate = self.sample_rate.ok_or_else(|| {
                    SophonError::SynthesisFailed(
                        "provider emitted audio before its stream format".into(),
                    )
                })?;
                if samples.is_empty() {
                    return Ok(());
                }
                if samples.iter().any(|sample| !sample.is_finite()) {
                    return Err(SophonError::SynthesisFailed(
                        "provider emitted a non-finite audio sample".into(),
                    ));
                }
                self.accepted_samples += samples.len() as u128;
                if self.accepted_samples
                    > u128::from(rate) * u128::from(self.max_generated_audio_seconds)
                {
                    return Err(SophonError::ResourceLimit(format!(
                        "generated audio exceeds {} seconds",
                        self.max_generated_audio_seconds
                    )));
                }
            }
            TtsStreamEvent::Terminal(_) => {
                return Err(SophonError::SynthesisFailed(
                    "provider emitted a reserved terminal event".into(),
                ));
            }
        }
        Ok(())
    }

    fn finish(&self) -> Result<(), SophonError> {
        if self.sample_rate.is_none() || self.accepted_samples == 0 {
            Err(SophonError::SynthesisFailed(
                "provider returned no streamed audio".into(),
            ))
        } else {
            Ok(())
        }
    }
}

fn handoff_stream_event(
    events: &tokio::sync::mpsc::Sender<TtsStreamEvent>,
    event: TtsStreamEvent,
) -> Result<(), SophonError> {
    let send = |event| {
        events.blocking_send(event).map_err(|_| {
            SophonError::SynthesisFailed("streaming synthesis consumer cancelled".into())
        })
    };
    match event {
        TtsStreamEvent::Format { sample_rate } => send(TtsStreamEvent::Format { sample_rate }),
        TtsStreamEvent::Chunk { samples } => {
            for chunk in samples.chunks(STREAM_CHUNK_SAMPLES) {
                if !chunk.is_empty() {
                    send(TtsStreamEvent::Chunk {
                        samples: chunk.to_vec(),
                    })?;
                }
            }
            Ok(())
        }
        TtsStreamEvent::Terminal(_) => Err(SophonError::SynthesisFailed(
            "provider emitted a reserved terminal event".into(),
        )),
    }
}

fn validate_generated_audio(
    audio: OwnedAudio,
    max_generated_audio_seconds: u64,
) -> Result<OwnedAudio, SophonError> {
    if audio.sample_rate == 0 {
        return Err(SophonError::SynthesisFailed(
            "provider returned a zero sample rate".into(),
        ));
    }
    if audio.samples.iter().any(|sample| !sample.is_finite()) {
        return Err(SophonError::SynthesisFailed(
            "provider returned a non-finite audio sample".into(),
        ));
    }
    let maximum_frames = u128::from(audio.sample_rate) * u128::from(max_generated_audio_seconds);
    if audio.samples.len() as u128 > maximum_frames {
        return Err(SophonError::ResourceLimit(format!(
            "generated audio exceeds {max_generated_audio_seconds} seconds"
        )));
    }
    Ok(audio)
}

#[derive(Clone)]
pub struct TtsWorker {
    sender: SyncSender<TtsWorkItem>,
    provider_id: &'static str,
    model_id: String,
    capabilities: TtsCapabilities,
    voices: Vec<String>,
}

impl TtsWorker {
    pub fn new(
        mut provider: Box<dyn TtsProvider>,
        capacity: usize,
        max_generated_audio_seconds: u64,
    ) -> Self {
        let provider_id = provider.provider_id();
        let model_id = provider.model_id().to_owned();
        let capabilities = provider.capabilities();
        let voices = provider.voices().to_vec();
        let (sender, receiver) = sync_channel::<TtsWorkItem>(capacity);
        thread::Builder::new()
            .name("sophon-tts-worker".into())
            .spawn(move || {
                while let Ok(work) = receiver.recv() {
                    match work {
                        TtsWorkItem::Buffered { request, response } => {
                            let result = provider.synthesize(&request).and_then(|audio| {
                                validate_generated_audio(audio, max_generated_audio_seconds)
                            });
                            let _ = response.send(result);
                        }
                        TtsWorkItem::Streaming {
                            request,
                            events,
                            terminal,
                        } => {
                            let result = if provider.supports_streaming() {
                                let mut validator =
                                    StreamValidator::new(max_generated_audio_seconds);
                                let mut callback_failure = None;
                                let provider_result = {
                                    let mut emit = |event| {
                                        if callback_failure.is_some() {
                                            return TtsStreamControl::Cancel;
                                        }
                                        if let Err(error) = validator
                                            .accept(&event)
                                            .and_then(|()| handoff_stream_event(&events, event))
                                        {
                                            callback_failure = Some(error);
                                            TtsStreamControl::Cancel
                                        } else {
                                            TtsStreamControl::Continue
                                        }
                                    };
                                    provider.synthesize_streaming(&request, &mut emit)
                                };
                                callback_failure
                                    .map_or(provider_result, Err)
                                    .and_then(|()| validator.finish())
                            } else {
                                provider
                                    .synthesize(&request)
                                    .and_then(|audio| {
                                        validate_generated_audio(audio, max_generated_audio_seconds)
                                    })
                                    .and_then(|audio| {
                                        handoff_stream_event(
                                            &events,
                                            TtsStreamEvent::Format {
                                                sample_rate: audio.sample_rate,
                                            },
                                        )?;
                                        handoff_stream_event(
                                            &events,
                                            TtsStreamEvent::Chunk {
                                                samples: audio.samples,
                                            },
                                        )
                                    })
                            };
                            let _ = terminal.send(result);
                        }
                    }
                }
            })
            .expect("failed to create TTS worker");
        Self {
            sender,
            provider_id,
            model_id,
            capabilities,
            voices,
        }
    }

    pub fn provider_id(&self) -> &'static str {
        self.provider_id
    }

    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    pub fn capabilities(&self) -> TtsCapabilities {
        self.capabilities
    }

    pub fn voices(&self) -> &[String] {
        &self.voices
    }

    pub async fn synthesize(&self, request: TtsRequest) -> Result<OwnedAudio, SophonError> {
        let (response, receiver) = oneshot::channel();
        match self
            .sender
            .try_send(TtsWorkItem::Buffered { request, response })
        {
            Ok(()) => receiver
                .await
                .map_err(|_| SophonError::SynthesisFailed("TTS worker stopped".into()))?,
            Err(TrySendError::Full(_)) => Err(SophonError::ResourceLimit(
                "TTS inference queue is full".into(),
            )),
            Err(TrySendError::Disconnected(_)) => Err(SophonError::ModelUnavailable(
                "TTS worker is unavailable".into(),
            )),
        }
    }

    pub fn synthesize_streaming(&self, request: TtsRequest) -> Result<TtsStream, SophonError> {
        let (events, receiver) = tokio::sync::mpsc::channel(STREAM_CHANNEL_EVENTS);
        let (terminal, terminal_receiver) = oneshot::channel();
        match self.sender.try_send(TtsWorkItem::Streaming {
            request,
            events,
            terminal,
        }) {
            Ok(()) => Ok(TtsStream {
                events: receiver,
                terminal_receiver: Some(terminal_receiver),
                terminal: None,
                terminal_emitted: false,
            }),
            Err(TrySendError::Full(_)) => Err(SophonError::ResourceLimit(
                "TTS inference queue is full".into(),
            )),
            Err(TrySendError::Disconnected(_)) => Err(SophonError::ModelUnavailable(
                "TTS worker is unavailable".into(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tts::VoiceIntent;
    use std::{
        sync::{
            Arc, Mutex,
            atomic::{AtomicBool, AtomicUsize, Ordering},
        },
        time::Duration,
    };

    struct FixtureProvider {
        calls: Arc<Mutex<Vec<TtsRequest>>>,
        fail_first: bool,
        voices: Vec<String>,
    }

    impl TtsProvider for FixtureProvider {
        fn provider_id(&self) -> &'static str {
            "fixture"
        }

        fn model_id(&self) -> &str {
            "fixture-model"
        }

        fn capabilities(&self) -> TtsCapabilities {
            capabilities()
        }

        fn voices(&self) -> &[String] {
            &self.voices
        }

        fn synthesize(&mut self, request: &TtsRequest) -> Result<OwnedAudio, SophonError> {
            self.calls.lock().unwrap().push(request.clone());
            if self.fail_first {
                self.fail_first = false;
                return Err(SophonError::SynthesisFailed("fixture failure".into()));
            }
            std::thread::sleep(Duration::from_millis(20));
            let samples = match request.text.as_str() {
                "oversize" => vec![request.speed as f32; 11],
                "large" => (0..(STREAM_CHUNK_SAMPLES * 2 + 1))
                    .map(|sample| sample as f32)
                    .collect(),
                _ => vec![request.speed as f32],
            };
            Ok(OwnedAudio {
                samples,
                sample_rate: 10,
            })
        }
    }

    struct StreamingFixtureProvider {
        buffered_calls: Arc<Mutex<Vec<String>>>,
        streaming_calls: Arc<Mutex<Vec<String>>>,
        cancelled: Arc<AtomicBool>,
    }

    impl TtsProvider for StreamingFixtureProvider {
        fn provider_id(&self) -> &'static str {
            "streaming-fixture"
        }

        fn model_id(&self) -> &str {
            "streaming-fixture-model"
        }

        fn capabilities(&self) -> TtsCapabilities {
            capabilities()
        }

        fn voices(&self) -> &[String] {
            &[]
        }

        fn synthesize(&mut self, request: &TtsRequest) -> Result<OwnedAudio, SophonError> {
            self.buffered_calls
                .lock()
                .unwrap()
                .push(request.text.clone());
            Ok(OwnedAudio {
                samples: vec![9.0],
                sample_rate: 10,
            })
        }

        fn supports_streaming(&self) -> bool {
            true
        }

        fn synthesize_streaming(
            &mut self,
            request: &TtsRequest,
            emit: &mut (dyn FnMut(TtsStreamEvent) -> TtsStreamControl + Send),
        ) -> Result<(), SophonError> {
            self.streaming_calls
                .lock()
                .unwrap()
                .push(request.text.clone());
            for event in [
                TtsStreamEvent::Format { sample_rate: 10 },
                TtsStreamEvent::Chunk { samples: vec![1.0] },
            ] {
                if emit(event) == TtsStreamControl::Cancel {
                    self.cancelled.store(true, Ordering::Release);
                    return Err(SophonError::SynthesisFailed("fixture cancelled".into()));
                }
            }
            std::thread::sleep(Duration::from_millis(20));
            if request.text == "fail" {
                return Err(SophonError::SynthesisFailed("fixture failure".into()));
            }
            let samples = if request.text == "overflow" {
                vec![2.0; 10]
            } else {
                vec![2.0]
            };
            if emit(TtsStreamEvent::Chunk { samples }) == TtsStreamControl::Cancel {
                self.cancelled.store(true, Ordering::Release);
                return Err(SophonError::SynthesisFailed("fixture cancelled".into()));
            }
            Ok(())
        }
    }

    struct BackpressureProvider {
        emitted: Arc<AtomicUsize>,
        cancelled: Arc<AtomicBool>,
    }

    impl TtsProvider for BackpressureProvider {
        fn provider_id(&self) -> &'static str {
            "backpressure-fixture"
        }

        fn model_id(&self) -> &str {
            "backpressure-fixture-model"
        }

        fn capabilities(&self) -> TtsCapabilities {
            capabilities()
        }

        fn voices(&self) -> &[String] {
            &[]
        }

        fn synthesize(&mut self, _: &TtsRequest) -> Result<OwnedAudio, SophonError> {
            Ok(OwnedAudio {
                samples: vec![1.0],
                sample_rate: 10,
            })
        }

        fn supports_streaming(&self) -> bool {
            true
        }

        fn synthesize_streaming(
            &mut self,
            _: &TtsRequest,
            emit: &mut (dyn FnMut(TtsStreamEvent) -> TtsStreamControl + Send),
        ) -> Result<(), SophonError> {
            if emit(TtsStreamEvent::Format { sample_rate: 10 }) == TtsStreamControl::Cancel {
                self.cancelled.store(true, Ordering::Release);
                return Err(SophonError::SynthesisFailed("fixture cancelled".into()));
            }
            for sample in 0..10 {
                if emit(TtsStreamEvent::Chunk {
                    samples: vec![sample as f32],
                }) == TtsStreamControl::Cancel
                {
                    self.cancelled.store(true, Ordering::Release);
                    return Err(SophonError::SynthesisFailed("fixture cancelled".into()));
                }
                self.emitted.fetch_add(1, Ordering::Release);
            }
            Ok(())
        }
    }

    fn streaming_fixture(
        buffered_calls: Arc<Mutex<Vec<String>>>,
        streaming_calls: Arc<Mutex<Vec<String>>>,
        cancelled: Arc<AtomicBool>,
    ) -> Box<dyn TtsProvider> {
        Box::new(StreamingFixtureProvider {
            buffered_calls,
            streaming_calls,
            cancelled,
        })
    }

    fn capabilities() -> TtsCapabilities {
        TtsCapabilities {
            named_voices: true,
            voice_cloning: false,
            voice_design: false,
            speed_control: true,
        }
    }

    fn request(text: &str, voice: VoiceIntent) -> TtsRequest {
        TtsRequest {
            text: text.into(),
            language: Some("en".into()),
            speed: 1.25,
            voice,
        }
    }

    fn fixture(calls: Arc<Mutex<Vec<TtsRequest>>>, fail_first: bool) -> Box<dyn TtsProvider> {
        Box::new(FixtureProvider {
            calls,
            fail_first,
            voices: vec!["af_heart".into(), "am_adam".into()],
        })
    }

    #[test]
    fn handoff_splits_large_chunks_without_reordering() {
        let (sender, mut receiver) = tokio::sync::mpsc::channel(STREAM_CHANNEL_EVENTS);
        let original: Vec<f32> = (0..(STREAM_CHUNK_SAMPLES * 2 + 1))
            .map(|sample| sample as f32)
            .collect();
        handoff_stream_event(
            &sender,
            TtsStreamEvent::Format {
                sample_rate: 24_000,
            },
        )
        .unwrap();
        handoff_stream_event(
            &sender,
            TtsStreamEvent::Chunk {
                samples: original.clone(),
            },
        )
        .unwrap();

        assert!(matches!(
            receiver.try_recv(),
            Ok(TtsStreamEvent::Format { .. })
        ));
        let mut actual = Vec::new();
        for expected_len in [STREAM_CHUNK_SAMPLES, STREAM_CHUNK_SAMPLES, 1] {
            let TtsStreamEvent::Chunk { samples } = receiver.try_recv().unwrap() else {
                panic!("handoff emitted a non-chunk event");
            };
            assert_eq!(samples.len(), expected_len);
            actual.extend(samples);
        }
        assert_eq!(actual, original);
    }

    #[test]
    fn blocking_stream_receiver_keeps_terminal_after_audio() {
        let mut stream = TtsStream::from_events([
            TtsStreamEvent::Format {
                sample_rate: 24_000,
            },
            TtsStreamEvent::Chunk {
                samples: vec![0.25],
            },
            TtsStreamEvent::Terminal(Ok(())),
        ]);
        assert!(matches!(
            stream.blocking_next(),
            Some(TtsStreamEvent::Format {
                sample_rate: 24_000
            })
        ));
        assert!(matches!(
            stream.blocking_next(),
            Some(TtsStreamEvent::Chunk { .. })
        ));
        assert!(matches!(
            stream.blocking_next(),
            Some(TtsStreamEvent::Terminal(Ok(())))
        ));
        assert!(stream.blocking_next().is_none());
    }

    #[test]
    fn terminal_error_is_observable_while_handoff_events_remain_queued() {
        let (sender, receiver) = tokio::sync::mpsc::channel(STREAM_CHANNEL_EVENTS);
        for event in [
            TtsStreamEvent::Format {
                sample_rate: 24_000,
            },
            TtsStreamEvent::Chunk { samples: vec![0.0] },
            TtsStreamEvent::Chunk { samples: vec![1.0] },
            TtsStreamEvent::Chunk { samples: vec![2.0] },
        ] {
            sender.try_send(event).unwrap();
        }
        let (terminal_sender, terminal_receiver) = oneshot::channel();
        terminal_sender
            .send(Err(SophonError::SynthesisFailed("fixture failure".into())))
            .unwrap();
        let mut stream = TtsStream {
            events: receiver,
            terminal_receiver: Some(terminal_receiver),
            terminal: None,
            terminal_emitted: false,
        };

        assert!(matches!(
            stream.try_terminal(),
            Ok(Some(Err(SophonError::SynthesisFailed(message)))) if message == "fixture failure"
        ));
        assert!(matches!(
            stream.blocking_next(),
            Some(TtsStreamEvent::Format {
                sample_rate: 24_000
            })
        ));
        drop(sender);
    }

    #[tokio::test]
    async fn stream_public_receivers_keep_terminal_after_audio() {
        let mut stream = TtsStream::from_events([
            TtsStreamEvent::Format {
                sample_rate: 24_000,
            },
            TtsStreamEvent::Chunk {
                samples: vec![0.25],
            },
            TtsStreamEvent::Terminal(Ok(())),
        ]);
        assert!(matches!(
            stream.next().await,
            Some(TtsStreamEvent::Format {
                sample_rate: 24_000
            })
        ));
        assert!(matches!(
            stream.next().await,
            Some(TtsStreamEvent::Chunk { .. })
        ));
        assert!(matches!(
            stream.next().await,
            Some(TtsStreamEvent::Terminal(Ok(())))
        ));
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn worker_preserves_request_options_fifo_and_recovers_after_failure() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let worker = TtsWorker::new(fixture(calls.clone(), true), 2, 10);
        assert_eq!(worker.provider_id(), "fixture");
        assert_eq!(worker.model_id(), "fixture-model");
        assert_eq!(worker.voices(), ["af_heart", "am_adam"]);
        assert!(matches!(
            worker
                .synthesize(request("first", VoiceIntent::Default))
                .await,
            Err(SophonError::SynthesisFailed(_))
        ));
        let second_request = request("second", VoiceIntent::Named("am_adam".into()));
        let third_request = request("third", VoiceIntent::Default);
        let (second, third) = tokio::join!(
            worker.synthesize(second_request.clone()),
            worker.synthesize(third_request.clone())
        );
        assert_eq!(second.unwrap().samples, vec![1.25]);
        assert!(third.is_ok());
        assert_eq!(
            *calls.lock().unwrap(),
            vec![
                request("first", VoiceIntent::Default),
                second_request,
                third_request
            ]
        );
    }

    #[tokio::test]
    async fn worker_streams_early_and_preserves_buffered_and_fallback_routing() {
        let buffered_calls = Arc::new(Mutex::new(Vec::new()));
        let streaming_calls = Arc::new(Mutex::new(Vec::new()));
        let worker = TtsWorker::new(
            streaming_fixture(
                buffered_calls.clone(),
                streaming_calls.clone(),
                Arc::new(AtomicBool::new(false)),
            ),
            2,
            1,
        );

        let buffered = worker
            .synthesize(request("buffered", VoiceIntent::Default))
            .await
            .unwrap();
        assert_eq!(buffered.samples, [9.0]);
        assert_eq!(*buffered_calls.lock().unwrap(), ["buffered"]);
        assert!(streaming_calls.lock().unwrap().is_empty());

        let mut stream = worker
            .synthesize_streaming(request("streamed", VoiceIntent::Default))
            .unwrap();
        assert!(matches!(
            stream.next().await,
            Some(TtsStreamEvent::Format { sample_rate: 10 })
        ));
        assert!(matches!(
            stream.next().await,
            Some(TtsStreamEvent::Chunk { samples }) if samples == [1.0]
        ));
        assert!(
            tokio::time::timeout(Duration::from_millis(5), stream.next())
                .await
                .is_err()
        );
        assert!(matches!(
            stream.next().await,
            Some(TtsStreamEvent::Chunk { samples }) if samples == [2.0]
        ));
        assert!(matches!(
            stream.next().await,
            Some(TtsStreamEvent::Terminal(Ok(())))
        ));
        assert_eq!(*streaming_calls.lock().unwrap(), ["streamed"]);

        let fallback_calls = Arc::new(Mutex::new(Vec::new()));
        let fallback = TtsWorker::new(fixture(fallback_calls.clone(), false), 1, 1);
        let mut stream = fallback
            .synthesize_streaming(request("fallback", VoiceIntent::Default))
            .unwrap();
        assert!(matches!(
            stream.next().await,
            Some(TtsStreamEvent::Format { sample_rate: 10 })
        ));
        assert!(matches!(
            stream.next().await,
            Some(TtsStreamEvent::Chunk { samples }) if samples == [1.25]
        ));
        assert!(matches!(
            stream.next().await,
            Some(TtsStreamEvent::Terminal(Ok(())))
        ));
        assert_eq!(fallback_calls.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn buffered_fallback_splits_large_chunks_without_reordering() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let worker = TtsWorker::new(fixture(calls, false), 1, 1_000);
        let mut stream = worker
            .synthesize_streaming(request("large", VoiceIntent::Default))
            .unwrap();
        assert!(matches!(
            stream.next().await,
            Some(TtsStreamEvent::Format { sample_rate: 10 })
        ));

        let mut samples = Vec::new();
        while let Some(event) = stream.next().await {
            match event {
                TtsStreamEvent::Chunk { samples: chunk } => {
                    assert!(chunk.len() <= STREAM_CHUNK_SAMPLES);
                    samples.extend(chunk);
                }
                TtsStreamEvent::Terminal(result) => {
                    result.unwrap();
                    break;
                }
                TtsStreamEvent::Format { .. } => panic!("fallback emitted a second format"),
            }
        }
        assert_eq!(
            samples,
            (0..(STREAM_CHUNK_SAMPLES * 2 + 1))
                .map(|sample| sample as f32)
                .collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn bounded_handoff_blocks_and_resumes_in_order() {
        let emitted = Arc::new(AtomicUsize::new(0));
        let worker = TtsWorker::new(
            Box::new(BackpressureProvider {
                emitted: emitted.clone(),
                cancelled: Arc::new(AtomicBool::new(false)),
            }),
            1,
            10,
        );
        let mut stream = worker
            .synthesize_streaming(request("backpressure", VoiceIntent::Default))
            .unwrap();
        tokio::time::timeout(Duration::from_millis(100), async {
            while emitted.load(Ordering::Acquire) != STREAM_CHANNEL_EVENTS - 1 {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .expect("producer filled the bounded handoff");
        assert!(matches!(
            stream.next().await,
            Some(TtsStreamEvent::Format { sample_rate: 10 })
        ));
        tokio::time::timeout(Duration::from_millis(100), async {
            while emitted.load(Ordering::Acquire) != STREAM_CHANNEL_EVENTS {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .expect("consumer capacity resumed the producer");

        let mut samples = Vec::new();
        while let Some(event) = stream.next().await {
            match event {
                TtsStreamEvent::Chunk { samples: chunk } => samples.extend(chunk),
                TtsStreamEvent::Terminal(result) => {
                    result.unwrap();
                    break;
                }
                TtsStreamEvent::Format { .. } => panic!("provider emitted a second format"),
            }
        }
        assert_eq!(
            samples,
            (0..10).map(|sample| sample as f32).collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn bounded_handoff_blocks_and_drop_unblocks_the_worker() {
        let emitted = Arc::new(AtomicUsize::new(0));
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker = TtsWorker::new(
            Box::new(BackpressureProvider {
                emitted: emitted.clone(),
                cancelled: cancelled.clone(),
            }),
            1,
            10,
        );
        let stream = worker
            .synthesize_streaming(request("backpressure", VoiceIntent::Default))
            .unwrap();
        tokio::time::timeout(Duration::from_millis(100), async {
            while emitted.load(Ordering::Acquire) != STREAM_CHANNEL_EVENTS - 1 {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .expect("producer filled the bounded handoff");
        assert_eq!(emitted.load(Ordering::Acquire), STREAM_CHANNEL_EVENTS - 1);
        drop(stream);
        tokio::time::timeout(Duration::from_millis(100), async {
            while !cancelled.load(Ordering::Acquire) {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .expect("dropping the consumer unblocked generation");
        assert!(
            worker
                .synthesize(request("recovered", VoiceIntent::Default))
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn worker_cancels_overflow_or_dropped_consumers_and_recovers() {
        let buffered_calls = Arc::new(Mutex::new(Vec::new()));
        let streaming_calls = Arc::new(Mutex::new(Vec::new()));
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker = TtsWorker::new(
            streaming_fixture(buffered_calls.clone(), streaming_calls, cancelled.clone()),
            2,
            1,
        );

        let mut overflow = worker
            .synthesize_streaming(request("overflow", VoiceIntent::Default))
            .unwrap();
        loop {
            if let Some(TtsStreamEvent::Terminal(result)) = overflow.next().await {
                assert!(matches!(result, Err(SophonError::ResourceLimit(_))));
                break;
            }
        }
        assert!(cancelled.load(Ordering::Acquire));
        cancelled.store(false, Ordering::Release);

        let mut failed = worker
            .synthesize_streaming(request("fail", VoiceIntent::Default))
            .unwrap();
        loop {
            if let Some(TtsStreamEvent::Terminal(result)) = failed.next().await {
                assert!(matches!(result, Err(SophonError::SynthesisFailed(_))));
                break;
            }
        }

        let mut dropped = worker
            .synthesize_streaming(request("cancel", VoiceIntent::Default))
            .unwrap();
        let _ = dropped.next().await;
        let _ = dropped.next().await;
        drop(dropped);
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert!(cancelled.load(Ordering::Acquire));

        assert!(
            worker
                .synthesize(request("recovered", VoiceIntent::Default))
                .await
                .is_ok()
        );
        assert_eq!(*buffered_calls.lock().unwrap(), ["recovered"]);
    }

    #[tokio::test]
    async fn worker_rejects_full_queue_and_oversized_output() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let worker = TtsWorker::new(fixture(calls, false), 1, 1);
        let first = worker.synthesize(request("first", VoiceIntent::Default));
        tokio::pin!(first);
        tokio::select! {
            _ = &mut first => panic!("fixture inference should not be instant"),
            _ = tokio::time::sleep(Duration::from_millis(1)) => {}
        }
        let queued = worker.synthesize(request("second", VoiceIntent::Default));
        tokio::pin!(queued);
        tokio::select! {
            _ = &mut queued => panic!("queued inference should wait"),
            _ = tokio::time::sleep(Duration::from_millis(1)) => {}
        }
        assert!(matches!(
            worker
                .synthesize(request("third", VoiceIntent::Default))
                .await,
            Err(SophonError::ResourceLimit(_))
        ));
        assert!(first.await.is_ok());
        assert!(queued.await.is_ok());
        assert!(matches!(
            worker
                .synthesize(request("oversize", VoiceIntent::Default))
                .await,
            Err(SophonError::ResourceLimit(_))
        ));
    }
}
