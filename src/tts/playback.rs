//! CPAL PipeWire playback and serialized playback scheduling.

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc::{SyncSender, TrySendError, sync_channel},
    },
    thread,
    time::Duration,
};

#[cfg(test)]
use std::collections::VecDeque;

use cpal::{
    DeviceId, HostId, Stream, StreamConfig,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use tokio::sync::oneshot;

use crate::{
    error::SophonError,
    tts::{TtsStream, TtsStreamEvent},
};

pub struct PlaybackRequest {
    pub stream: TtsStream,
    pub output_device: Option<DeviceId>,
    pub volume: f32,
}

pub trait SpeechPlayback: Send {
    /// Consumes one logical mono float stream and returns after its final frame drains.
    fn play(&mut self, request: PlaybackRequest) -> Result<(), SophonError>;
}

struct PlaybackWorkItem {
    request: PlaybackRequest,
    response: oneshot::Sender<Result<(), SophonError>>,
}

#[derive(Clone)]
pub struct PlaybackWorker {
    sender: SyncSender<PlaybackWorkItem>,
}

impl PlaybackWorker {
    pub fn new(mut playback: Box<dyn SpeechPlayback>, capacity: usize) -> Self {
        let (sender, receiver) = sync_channel::<PlaybackWorkItem>(capacity);
        thread::Builder::new()
            .name("sophon-playback-worker".into())
            .spawn(move || {
                while let Ok(work) = receiver.recv() {
                    let _ = work.response.send(playback.play(work.request));
                }
            })
            .expect("failed to create playback worker");
        Self { sender }
    }

    pub async fn play(&self, request: PlaybackRequest) -> Result<(), SophonError> {
        let (response, receiver) = oneshot::channel();
        match self.sender.try_send(PlaybackWorkItem { request, response }) {
            Ok(()) => receiver
                .await
                .map_err(|_| SophonError::PlaybackFailed("playback worker stopped".into()))?,
            Err(TrySendError::Full(_)) => Err(SophonError::ResourceLimit(
                "speech playback queue is full".into(),
            )),
            Err(TrySendError::Disconnected(_)) => Err(SophonError::PlaybackFailed(
                "playback worker is unavailable".into(),
            )),
        }
    }
}

fn select_device<T>(
    configured: Option<&DeviceId>,
    default: impl FnOnce() -> Option<T>,
    exact: impl FnOnce(&DeviceId) -> Option<T>,
) -> Result<T, SophonError> {
    match configured {
        Some(id) => exact(id).ok_or_else(|| {
            SophonError::PlaybackFailed(format!(
                "configured CPAL output device `{id}` is unavailable"
            ))
        }),
        None => default().ok_or_else(|| {
            SophonError::PlaybackFailed("PipeWire has no default output device".into())
        }),
    }
}

#[derive(Debug)]
struct SampleRing {
    samples: Vec<f32>,
    read: usize,
    write: usize,
    len: usize,
    generation: u64,
}

impl SampleRing {
    fn new(capacity: usize) -> Self {
        Self {
            samples: vec![0.0; capacity.max(1)],
            read: 0,
            write: 0,
            len: 0,
            generation: 0,
        }
    }

    fn available(&self) -> usize {
        self.samples.len() - self.len
    }

    fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn push_slice(&mut self, input: &[f32]) -> usize {
        let count = input.len().min(self.available());
        for sample in &input[..count] {
            self.samples[self.write] = *sample;
            self.write = (self.write + 1) % self.samples.len();
        }
        self.len += count;
        if count != 0 {
            self.generation = self.generation.wrapping_add(1);
        }
        count
    }

    fn pop(&mut self) -> Option<f32> {
        if self.len == 0 {
            return None;
        }
        let sample = self.samples[self.read];
        self.read = (self.read + 1) % self.samples.len();
        self.len -= 1;
        Some(sample)
    }
}

#[cfg(test)]
fn fill_ring_from_chunks(ring: &mut SampleRing, chunks: &mut VecDeque<(Vec<f32>, usize)>) {
    while let Some((samples, offset)) = chunks.front_mut() {
        let pushed = ring.push_slice(&samples[*offset..]);
        *offset += pushed;
        if *offset == samples.len() {
            chunks.pop_front();
        }
        if pushed == 0 {
            break;
        }
    }
}

fn fill_ring_from_pending(ring: &mut SampleRing, pending: &mut Option<(Vec<f32>, usize)>) {
    let Some((samples, offset)) = pending.as_mut() else {
        return;
    };
    *offset += ring.push_slice(&samples[*offset..]);
    if *offset == samples.len() {
        *pending = None;
    }
}

fn drain_complete(
    chunks_empty: bool,
    ring_empty: bool,
    ring_generation: u64,
    submitted_generation: u64,
    deadline_nanos: u64,
    now_nanos: u128,
) -> bool {
    chunks_empty
        && ring_empty
        && ring_generation == submitted_generation
        && deadline_nanos != 0
        && now_nanos >= u128::from(deadline_nanos)
}

fn render_output(
    output: &mut [f32],
    channels: usize,
    volume: f32,
    ring: Option<&mut SampleRing>,
) -> usize {
    output.fill(0.0);
    if channels == 0 {
        return 0;
    }
    let Some(ring) = ring else {
        return 0;
    };
    let mut consumed = 0;
    for frame in output.chunks_exact_mut(channels) {
        let Some(sample) = ring.pop() else {
            break;
        };
        frame.fill(sample * volume);
        consumed += 1;
    }
    consumed
}

struct CpalSession {
    _stream: Stream,
    ring: Arc<Mutex<SampleRing>>,
    deadline_nanos: Arc<AtomicU64>,
    submitted_generation: Arc<AtomicU64>,
    failure: Arc<Mutex<Option<String>>>,
}

impl CpalSession {
    fn drain_state(&self) -> (bool, u64) {
        let ring = self
            .ring
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        (ring.is_empty(), ring.generation)
    }

    fn failure(&self) -> Option<String> {
        self.failure
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

#[derive(Debug, Default)]
pub struct CpalPlayback;

impl CpalPlayback {
    const RING_FRAMES: usize = 4_096;

    fn playback_error(error: impl std::fmt::Display) -> SophonError {
        SophonError::PlaybackFailed(error.to_string())
    }

    fn open_session(
        &self,
        output_device: Option<&DeviceId>,
        sample_rate: u32,
        volume: f32,
    ) -> Result<CpalSession, SophonError> {
        if sample_rate == 0 {
            return Err(SophonError::PlaybackFailed(
                "playback requires a nonzero sample rate".into(),
            ));
        }
        let host = cpal::host_from_id(HostId::PipeWire).map_err(Self::playback_error)?;
        let device = select_device(
            output_device,
            || host.default_output_device(),
            |id| host.device_by_id(id),
        )?;
        let default = device
            .default_output_config()
            .map_err(Self::playback_error)?;
        let channels = usize::from(default.channels());
        if channels == 0 {
            return Err(SophonError::PlaybackFailed(
                "selected output device has no channels".into(),
            ));
        }
        let config = StreamConfig {
            channels: default.channels(),
            sample_rate,
            buffer_size: cpal::BufferSize::Default,
        };
        let ring = Arc::new(Mutex::new(SampleRing::new(Self::RING_FRAMES)));
        let callback_ring = Arc::clone(&ring);
        let deadline_nanos = Arc::new(AtomicU64::new(0));
        let callback_deadline = Arc::clone(&deadline_nanos);
        let submitted_generation = Arc::new(AtomicU64::new(0));
        let callback_submitted_generation = Arc::clone(&submitted_generation);
        let failure = Arc::new(Mutex::new(None));
        let callback_failure = Arc::clone(&failure);
        let stream = device
            .build_output_stream::<f32, _, _>(
                config,
                move |output, info| {
                    let Ok(mut ring) = callback_ring.try_lock() else {
                        render_output(output, channels, volume, None);
                        return;
                    };
                    let consumed = render_output(output, channels, volume, Some(&mut ring));
                    if consumed != 0 {
                        let frames = output.len() / channels;
                        let duration = Duration::from_secs_f64(frames as f64 / sample_rate as f64);
                        if let Some(deadline) = info.timestamp().playback.checked_add(duration) {
                            let nanos = deadline.as_nanos().min(u128::from(u64::MAX)) as u64;
                            // Publish both values while still holding the ring lock. A
                            // completion observer can no longer see an empty ring paired
                            // with the previous callback's stale deadline.
                            callback_deadline.store(nanos, Ordering::Release);
                            callback_submitted_generation.store(ring.generation, Ordering::Release);
                        }
                    }
                },
                move |error| {
                    *callback_failure
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner) =
                        Some(error.to_string());
                },
                None,
            )
            .map_err(|error| {
                SophonError::PlaybackFailed(format!(
                    "cannot open {} Hz f32 output stream: {error}",
                    sample_rate
                ))
            })?;
        stream.play().map_err(Self::playback_error)?;
        Ok(CpalSession {
            _stream: stream,
            ring,
            deadline_nanos,
            submitted_generation,
            failure,
        })
    }

    fn fill_ring(session: &CpalSession, pending: &mut Option<(Vec<f32>, usize)>) {
        let mut ring = session
            .ring
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        fill_ring_from_pending(&mut ring, pending);
    }
}

impl SpeechPlayback for CpalPlayback {
    fn play(&mut self, mut request: PlaybackRequest) -> Result<(), SophonError> {
        if !request.volume.is_finite() || !(0.0..=1.0).contains(&request.volume) {
            return Err(SophonError::PlaybackFailed(
                "playback volume must be finite and between 0.0 and 1.0".into(),
            ));
        }
        if request
            .output_device
            .as_ref()
            .is_some_and(|id| id.host() != HostId::PipeWire || id.id().is_empty())
        {
            return Err(SophonError::PlaybackFailed(
                "output device must be a canonical PipeWire device ID".into(),
            ));
        }

        let mut sample_rate = None;
        let mut pending = None;
        let mut session: Option<CpalSession> = None;
        let mut terminal: Option<()> = None;

        loop {
            if terminal.is_none()
                && let Some(Err(error)) = request.stream.try_terminal()?
            {
                return Err(error);
            }
            if let Some(session) = &session {
                if let Some(error) = session.failure() {
                    return Err(SophonError::PlaybackFailed(error));
                }
                // Check the independent terminal result before adding pending samples.
                Self::fill_ring(session, &mut pending);
            }

            if session.is_none() && pending.is_some() {
                session = Some(self.open_session(
                    request.output_device.as_ref(),
                    sample_rate.expect("chunks require a format"),
                    request.volume,
                )?);
                continue;
            }

            // Keep no more than one nonempty handoff chunk outside the ring.
            while terminal.is_none() && pending.is_none() {
                match request.stream.try_next() {
                    Ok(TtsStreamEvent::Format { sample_rate: rate }) => {
                        if rate == 0 || sample_rate.replace(rate).is_some() {
                            return Err(SophonError::PlaybackFailed(
                                "synthesis supplied an invalid stream format".into(),
                            ));
                        }
                    }
                    Ok(TtsStreamEvent::Chunk { samples }) => {
                        if sample_rate.is_none() {
                            return Err(SophonError::PlaybackFailed(
                                "synthesis supplied audio before its format".into(),
                            ));
                        }
                        if samples.iter().any(|sample| !sample.is_finite()) {
                            return Err(SophonError::PlaybackFailed(
                                "playback audio contains a non-finite sample".into(),
                            ));
                        }
                        if !samples.is_empty() {
                            pending = Some((samples, 0));
                        }
                    }
                    Ok(TtsStreamEvent::Terminal(result)) => {
                        result?;
                        terminal = Some(());
                        break;
                    }
                    Err(tokio::sync::mpsc::error::TryRecvError::Empty) => break,
                    Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                        return Err(SophonError::SynthesisFailed(
                            "streaming synthesis stopped without a terminal event".into(),
                        ));
                    }
                }
            }

            if terminal.is_some() {
                let Some(session) = &session else {
                    return Err(SophonError::PlaybackFailed(
                        "synthesis completed without audio".into(),
                    ));
                };
                let (ring_empty, ring_generation) = session.drain_state();
                let submitted_generation = session.submitted_generation.load(Ordering::Acquire);
                let deadline = session.deadline_nanos.load(Ordering::Acquire);
                let now = session._stream.now().as_nanos();
                if drain_complete(
                    pending.is_none(),
                    ring_empty,
                    ring_generation,
                    submitted_generation,
                    deadline,
                    now,
                ) {
                    return Ok(());
                }
            }

            thread::sleep(Duration::from_millis(1));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        str::FromStr,
        sync::atomic::{AtomicUsize, Ordering as AtomicOrdering},
    };

    fn stream(events: impl IntoIterator<Item = TtsStreamEvent>) -> TtsStream {
        TtsStream::from_events(events)
    }

    #[test]
    fn device_policy_uses_default_or_exact_without_fallback() {
        assert_eq!(
            select_device(None, || Some("default"), |_| Some("exact")).unwrap(),
            "default"
        );
        let id = DeviceId::from_str("pipewire:test-device").unwrap();
        assert_eq!(
            select_device(Some(&id), || Some("default"), |_| Some("exact")).unwrap(),
            "exact"
        );
        assert!(select_device(Some(&id), || Some("default"), |_| None::<&str>).is_err());
        assert!(select_device(None, || None::<&str>, |_| Some("exact")).is_err());
    }

    #[test]
    fn callback_duplicates_volume_and_writes_silence_on_underrun() {
        let mut ring = SampleRing::new(4);
        assert_eq!(ring.push_slice(&[1.0, -0.5]), 2);
        let mut output = [9.0; 6];
        assert_eq!(render_output(&mut output, 2, 0.5, Some(&mut ring)), 2);
        assert_eq!(output, [0.5, 0.5, -0.25, -0.25, 0.0, 0.0]);
        assert!(ring.is_empty());
    }

    #[test]
    fn ring_is_bounded_and_fast_producer_chunks_refill_in_order() {
        let mut ring = SampleRing::new(3);
        let mut chunks = VecDeque::from([(vec![1.0, 2.0], 0), (vec![3.0, 4.0, 5.0], 0)]);
        let mut consumed = Vec::new();
        while !chunks.is_empty() || !ring.is_empty() {
            fill_ring_from_chunks(&mut ring, &mut chunks);
            while let Some(sample) = ring.pop() {
                consumed.push(sample);
            }
        }
        assert_eq!(consumed, [1.0, 2.0, 3.0, 4.0, 5.0]);
        assert_eq!(ring.generation, 3);
    }

    #[test]
    fn drain_requires_the_latest_ring_generation_and_playback_deadline() {
        assert!(!drain_complete(false, true, 2, 2, 100, 100));
        assert!(!drain_complete(true, false, 2, 2, 100, 100));
        assert!(!drain_complete(true, true, 2, 1, 100, 100));
        assert!(!drain_complete(true, true, 2, 2, 100, 99));
        assert!(!drain_complete(true, true, 2, 2, 0, 100));
        assert!(drain_complete(true, true, 2, 2, 100, 100));
    }

    struct FixturePlayback {
        calls: Arc<Mutex<Vec<Vec<f32>>>>,
        fail_first: bool,
    }

    impl SpeechPlayback for FixturePlayback {
        fn play(&mut self, mut request: PlaybackRequest) -> Result<(), SophonError> {
            let mut samples = Vec::new();
            while let Some(event) = request.stream.blocking_next() {
                match event {
                    TtsStreamEvent::Chunk { samples: chunk } => samples.extend(chunk),
                    TtsStreamEvent::Terminal(result) => {
                        result?;
                        break;
                    }
                    TtsStreamEvent::Format { .. } => {}
                }
            }
            self.calls.lock().unwrap().push(samples);
            if self.fail_first {
                self.fail_first = false;
                Err(SophonError::PlaybackFailed("fixture failure".into()))
            } else {
                Ok(())
            }
        }
    }

    #[tokio::test]
    async fn playback_worker_is_fifo_bounded_and_recovers() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let worker = PlaybackWorker::new(
            Box::new(FixturePlayback {
                calls: calls.clone(),
                fail_first: true,
            }),
            1,
        );
        let request = |sample| PlaybackRequest {
            stream: stream([
                TtsStreamEvent::Format {
                    sample_rate: 24_000,
                },
                TtsStreamEvent::Chunk {
                    samples: vec![sample],
                },
                TtsStreamEvent::Terminal(Ok(())),
            ]),
            output_device: None,
            volume: 1.0,
        };
        assert!(matches!(
            worker.play(request(1.0)).await,
            Err(SophonError::PlaybackFailed(_))
        ));
        assert!(worker.play(request(2.0)).await.is_ok());
        assert_eq!(*calls.lock().unwrap(), vec![vec![1.0], vec![2.0]]);
    }

    struct SerialPlayback {
        active: Arc<AtomicUsize>,
        maximum: Arc<AtomicUsize>,
        order: Arc<Mutex<Vec<f32>>>,
    }

    impl SpeechPlayback for SerialPlayback {
        fn play(&mut self, mut request: PlaybackRequest) -> Result<(), SophonError> {
            let active = self.active.fetch_add(1, AtomicOrdering::AcqRel) + 1;
            self.maximum.fetch_max(active, AtomicOrdering::AcqRel);
            while let Some(event) = request.stream.blocking_next() {
                match event {
                    TtsStreamEvent::Chunk { samples } => self.order.lock().unwrap().extend(samples),
                    TtsStreamEvent::Terminal(result) => {
                        result?;
                        break;
                    }
                    TtsStreamEvent::Format { .. } => {}
                }
            }
            thread::sleep(Duration::from_millis(10));
            self.active.fetch_sub(1, AtomicOrdering::AcqRel);
            Ok(())
        }
    }

    #[test]
    #[ignore = "run tests/pipewire-smoke.sh inside nix develop"]
    fn cpal_pipewire_smoke_opens_native_rate_and_drains() {
        let node = std::env::var("SOPHON_PIPEWIRE_SMOKE_NODE")
            .expect("SOPHON_PIPEWIRE_SMOKE_NODE must name the isolated sink");
        for output_device in [
            None,
            Some(DeviceId::from_str(&format!("pipewire:{node}")).unwrap()),
        ] {
            for level in [0.05, 0.1, 0.15] {
                let request = PlaybackRequest {
                    stream: stream([
                        TtsStreamEvent::Format {
                            sample_rate: 24_000,
                        },
                        TtsStreamEvent::Chunk {
                            samples: vec![level; 4_800],
                        },
                        TtsStreamEvent::Chunk {
                            samples: vec![-level; 4_800],
                        },
                        TtsStreamEvent::Terminal(Ok(())),
                    ]),
                    output_device: output_device.clone(),
                    volume: 0.5,
                };
                CpalPlayback.play(request).unwrap();
            }
        }
    }

    #[tokio::test]
    async fn playback_worker_serializes_concurrent_requests() {
        let active = Arc::new(AtomicUsize::new(0));
        let maximum = Arc::new(AtomicUsize::new(0));
        let order = Arc::new(Mutex::new(Vec::new()));
        let worker = PlaybackWorker::new(
            Box::new(SerialPlayback {
                active: active.clone(),
                maximum: maximum.clone(),
                order: order.clone(),
            }),
            2,
        );
        let request = |sample| PlaybackRequest {
            stream: stream([
                TtsStreamEvent::Format {
                    sample_rate: 24_000,
                },
                TtsStreamEvent::Chunk {
                    samples: vec![sample],
                },
                TtsStreamEvent::Terminal(Ok(())),
            ]),
            output_device: None,
            volume: 1.0,
        };
        let (first, second) = tokio::join!(worker.play(request(1.0)), worker.play(request(2.0)));
        assert!(first.is_ok());
        assert!(second.is_ok());
        assert_eq!(maximum.load(AtomicOrdering::Acquire), 1);
        assert_eq!(*order.lock().unwrap(), [1.0, 2.0]);
    }
}
