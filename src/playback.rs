//! Provider-neutral speech playback and serialized playback scheduling.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::{
        Arc,
        mpsc::{SyncSender, TrySendError, sync_channel},
    },
    thread,
};

use tokio::sync::oneshot;

use pipewire as pw;
use pw::{
    properties::properties,
    spa::{self, pod::Pod},
    types::ObjectType,
};

use crate::domain::{OwnedAudio, SophonError};

#[derive(Debug, Clone, PartialEq)]
pub struct PlaybackRequest {
    pub audio: OwnedAudio,
    pub node_name: Option<String>,
    pub volume: f32,
}

pub trait SpeechPlayback: Send {
    /// Plays all owned mono float PCM and returns only after the stream drains.
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

type NodeResolver = dyn Fn(&str) -> Result<u32, SophonError> + Send + Sync;

pub struct PipeWirePlayback {
    node_resolver: Arc<NodeResolver>,
}

impl std::fmt::Debug for PipeWirePlayback {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("PipeWirePlayback").finish()
    }
}

impl Default for PipeWirePlayback {
    fn default() -> Self {
        Self {
            node_resolver: Arc::new(Self::resolve_node_name),
        }
    }
}

struct PipeWireData {
    samples: Vec<f32>,
    position: usize,
    draining: bool,
}

impl PipeWirePlayback {
    fn error(error: impl std::fmt::Display) -> SophonError {
        SophonError::PlaybackFailed(error.to_string())
    }

    fn target_node(&self, name: Option<&str>) -> Result<Option<u32>, SophonError> {
        name.map(|name| (self.node_resolver)(name)).transpose()
    }

    fn prepare_request(mut request: PlaybackRequest) -> Result<PlaybackRequest, SophonError> {
        if request.audio.sample_rate == 0 || request.audio.samples.is_empty() {
            return Err(SophonError::PlaybackFailed(
                "playback requires non-empty audio with a nonzero sample rate".into(),
            ));
        }
        if !request.volume.is_finite() || !(0.0..=1.0).contains(&request.volume) {
            return Err(SophonError::PlaybackFailed(
                "playback volume must be finite and between 0.0 and 1.0".into(),
            ));
        }
        if request
            .audio
            .samples
            .iter()
            .any(|sample| !sample.is_finite())
        {
            return Err(SophonError::PlaybackFailed(
                "playback audio contains a non-finite sample".into(),
            ));
        }
        for sample in &mut request.audio.samples {
            *sample *= request.volume;
        }
        Ok(request)
    }

    fn resolve_node_name(name: &str) -> Result<u32, SophonError> {
        let mainloop = pw::main_loop::MainLoopRc::new(None).map_err(Self::error)?;
        let context = pw::context::ContextRc::new(&mainloop, None).map_err(Self::error)?;
        let core = context.connect_rc(None).map_err(Self::error)?;
        let registry = core.get_registry_rc().map_err(Self::error)?;
        let target = Rc::new(Cell::new(None));
        let target_for_listener = target.clone();
        let requested = name.to_owned();
        let _registry_listener = registry
            .add_listener_local()
            .global(move |object| {
                if object.type_ != ObjectType::Node {
                    return;
                }
                let Some(properties) = object.props else {
                    return;
                };
                if properties.get(*pw::keys::NODE_NAME) == Some(requested.as_str())
                    && properties.get(*pw::keys::MEDIA_CLASS) == Some("Audio/Sink")
                {
                    target_for_listener.set(Some(object.id));
                }
            })
            .register();
        let pending = core.sync(0).map_err(Self::error)?;
        let done = Rc::new(Cell::new(false));
        let done_for_listener = done.clone();
        let loop_for_listener = mainloop.clone();
        let failure = Rc::new(RefCell::new(None));
        let failure_for_listener = failure.clone();
        let _core_listener = core
            .add_listener_local()
            .done(move |id, sequence| {
                if id == pw::core::PW_ID_CORE && sequence == pending {
                    done_for_listener.set(true);
                    loop_for_listener.quit();
                }
            })
            .error({
                let loop_for_error = mainloop.clone();
                move |_id, _sequence, _result, message| {
                    *failure_for_listener.borrow_mut() = Some(message.to_owned());
                    loop_for_error.quit();
                }
            })
            .register();
        while !done.get() && failure.borrow().is_none() {
            mainloop.run();
        }
        if let Some(error) = failure.borrow_mut().take() {
            return Err(Self::error(error));
        }
        target.get().ok_or_else(|| {
            SophonError::PlaybackFailed(format!("PipeWire node.name `{name}` was not found"))
        })
    }
}

impl SpeechPlayback for PipeWirePlayback {
    fn play(&mut self, request: PlaybackRequest) -> Result<(), SophonError> {
        let request = Self::prepare_request(request)?;
        pw::init();
        let target = self.target_node(request.node_name.as_deref())?;
        let mainloop = pw::main_loop::MainLoopRc::new(None).map_err(Self::error)?;
        let context = pw::context::ContextRc::new(&mainloop, None).map_err(Self::error)?;
        let core = context.connect_rc(None).map_err(Self::error)?;
        let stream = pw::stream::StreamBox::new(
            &core,
            "sophon-tts-playback",
            properties! {
                *pw::keys::MEDIA_TYPE => "Audio",
                *pw::keys::MEDIA_CATEGORY => "Playback",
                *pw::keys::MEDIA_ROLE => "Accessibility",
                *pw::keys::AUDIO_CHANNELS => "1",
                *pw::keys::NODE_NAME => "sophon-tts-playback",
            },
        )
        .map_err(Self::error)?;

        let outcome = Rc::new(RefCell::new(None::<Result<(), String>>));
        let outcome_for_state = outcome.clone();
        let outcome_for_process = outcome.clone();
        let outcome_for_drain = outcome.clone();
        let loop_for_state = mainloop.clone();
        let loop_for_process = mainloop.clone();
        let loop_for_drain = mainloop.clone();
        let _listener = stream
            .add_local_listener_with_user_data(PipeWireData {
                samples: request.audio.samples,
                position: 0,
                draining: false,
            })
            .state_changed(move |_stream, _data, _old, new| {
                if let pw::stream::StreamState::Error(error) = new {
                    *outcome_for_state.borrow_mut() = Some(Err(error));
                    loop_for_state.quit();
                }
            })
            .process(move |stream, data| {
                if data.position >= data.samples.len() {
                    if !data.draining {
                        data.draining = true;
                        if let Err(error) = stream.flush(true) {
                            *outcome_for_process.borrow_mut() = Some(Err(error.to_string()));
                            loop_for_process.quit();
                        }
                    }
                    return;
                }
                let Some(mut buffer) = stream.dequeue_buffer() else {
                    return;
                };
                let Some(output) = buffer.datas_mut().first_mut().and_then(|data| data.data())
                else {
                    return;
                };
                let frames = (output.len() / std::mem::size_of::<f32>())
                    .min(data.samples.len() - data.position);
                for (destination, sample) in output
                    .chunks_exact_mut(std::mem::size_of::<f32>())
                    .zip(&data.samples[data.position..data.position + frames])
                {
                    destination.copy_from_slice(&sample.to_le_bytes());
                }
                data.position += frames;
                let chunk = buffer.datas_mut()[0].chunk_mut();
                *chunk.offset_mut() = 0;
                *chunk.stride_mut() = std::mem::size_of::<f32>() as i32;
                *chunk.size_mut() = (frames * std::mem::size_of::<f32>()) as u32;
            })
            .drained(move |_stream, _data| {
                *outcome_for_drain.borrow_mut() = Some(Ok(()));
                loop_for_drain.quit();
            })
            .register()
            .map_err(Self::error)?;

        let mut audio_info = spa::param::audio::AudioInfoRaw::new();
        audio_info.set_format(spa::param::audio::AudioFormat::F32LE);
        audio_info.set_rate(request.audio.sample_rate);
        audio_info.set_channels(1);
        let object = spa::pod::Object {
            type_: spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
            id: spa::param::ParamType::EnumFormat.as_raw(),
            properties: audio_info.into(),
        };
        let serialized = spa::pod::serialize::PodSerializer::serialize(
            std::io::Cursor::new(Vec::new()),
            &spa::pod::Value::Object(object),
        )
        .map_err(Self::error)?
        .0
        .into_inner();
        let mut parameters = [Pod::from_bytes(&serialized).ok_or_else(|| {
            SophonError::PlaybackFailed("could not construct PipeWire audio format".into())
        })?];
        stream
            .connect(
                spa::utils::Direction::Output,
                target,
                pw::stream::StreamFlags::AUTOCONNECT
                    | pw::stream::StreamFlags::MAP_BUFFERS
                    | pw::stream::StreamFlags::RT_PROCESS,
                &mut parameters,
            )
            .map_err(Self::error)?;
        while outcome.borrow().is_none() {
            mainloop.run();
        }
        match outcome.borrow_mut().take().expect("playback outcome set") {
            Ok(()) => Ok(()),
            Err(error) => Err(Self::error(error)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::{
            Mutex,
            atomic::{AtomicUsize, Ordering},
        },
        time::{Duration, Instant},
    };

    fn request(sample: f32, node_name: Option<&str>, volume: f32) -> PlaybackRequest {
        PlaybackRequest {
            audio: OwnedAudio {
                samples: vec![sample],
                sample_rate: 24_000,
            },
            node_name: node_name.map(str::to_owned),
            volume,
        }
    }

    #[test]
    fn pipewire_policy_scales_volume_and_resolves_only_explicit_exact_names() {
        let prepared = PipeWirePlayback::prepare_request(request(0.8, None, 0.5)).unwrap();
        assert_eq!(prepared.audio.samples, [0.4]);
        let muted = PipeWirePlayback::prepare_request(request(0.8, None, 0.0)).unwrap();
        assert_eq!(muted.audio.samples, [0.0]);

        let names = Arc::new(Mutex::new(Vec::new()));
        let names_for_resolver = names.clone();
        let playback = PipeWirePlayback {
            node_resolver: Arc::new(move |name| {
                names_for_resolver.lock().unwrap().push(name.to_owned());
                if name == "sink.ok" {
                    Ok(42)
                } else {
                    Err(SophonError::PlaybackFailed("missing fixture sink".into()))
                }
            }),
        };
        assert_eq!(playback.target_node(None).unwrap(), None);
        assert_eq!(playback.target_node(Some("sink.ok")).unwrap(), Some(42));
        assert!(matches!(
            playback.target_node(Some("sink.missing")),
            Err(SophonError::PlaybackFailed(_))
        ));
        assert_eq!(*names.lock().unwrap(), ["sink.ok", "sink.missing"]);
    }

    struct FixturePlayback {
        calls: Arc<Mutex<Vec<PlaybackRequest>>>,
        active: Arc<AtomicUsize>,
        maximum_active: Arc<AtomicUsize>,
        fail_first: bool,
    }

    impl SpeechPlayback for FixturePlayback {
        fn play(&mut self, request: PlaybackRequest) -> Result<(), SophonError> {
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.maximum_active.fetch_max(active, Ordering::SeqCst);
            self.calls.lock().unwrap().push(request);
            std::thread::sleep(Duration::from_millis(20));
            self.active.fetch_sub(1, Ordering::SeqCst);
            if self.fail_first {
                self.fail_first = false;
                Err(SophonError::PlaybackFailed("fixture failure".into()))
            } else {
                Ok(())
            }
        }
    }

    fn fixture(
        calls: Arc<Mutex<Vec<PlaybackRequest>>>,
        active: Arc<AtomicUsize>,
        maximum_active: Arc<AtomicUsize>,
        fail_first: bool,
    ) -> Box<dyn SpeechPlayback> {
        Box::new(FixturePlayback {
            calls,
            active,
            maximum_active,
            fail_first,
        })
    }

    #[tokio::test]
    async fn playback_worker_is_synchronous_fifo_serial_and_recovers_after_failure() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let active = Arc::new(AtomicUsize::new(0));
        let maximum_active = Arc::new(AtomicUsize::new(0));
        let worker = PlaybackWorker::new(
            fixture(calls.clone(), active.clone(), maximum_active.clone(), true),
            2,
        );
        let started = Instant::now();
        assert!(matches!(
            worker.play(request(1.0, None, 1.0)).await,
            Err(SophonError::PlaybackFailed(_))
        ));
        assert!(started.elapsed() >= Duration::from_millis(20));
        let (second, third) = tokio::join!(
            worker.play(request(2.0, Some("sink.ok"), 0.5)),
            worker.play(request(3.0, None, 1.0))
        );
        assert!(second.is_ok());
        assert!(third.is_ok());
        assert_eq!(active.load(Ordering::SeqCst), 0);
        assert_eq!(maximum_active.load(Ordering::SeqCst), 1);
        assert_eq!(
            calls
                .lock()
                .unwrap()
                .iter()
                .map(|request| request.audio.samples[0])
                .collect::<Vec<_>>(),
            [1.0, 2.0, 3.0]
        );
    }

    #[test]
    #[ignore = "run tests/pipewire-smoke.sh inside nix develop"]
    fn pipewire_smoke_negotiates_and_drains() {
        let node = std::env::var("SOPHON_PIPEWIRE_SMOKE_NODE")
            .expect("smoke harness must provide an exact PipeWire node.name");
        let mut playback = PipeWirePlayback::default();
        playback
            .play(PlaybackRequest {
                audio: OwnedAudio {
                    samples: vec![0.0; 2_400],
                    sample_rate: 24_000,
                },
                node_name: Some(node),
                volume: 1.0,
            })
            .unwrap();
    }

    #[tokio::test]
    async fn playback_worker_rejects_a_full_queue() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let active = Arc::new(AtomicUsize::new(0));
        let maximum_active = Arc::new(AtomicUsize::new(0));
        let worker = PlaybackWorker::new(fixture(calls, active, maximum_active, false), 1);
        let first = worker.play(request(1.0, None, 1.0));
        tokio::pin!(first);
        tokio::select! {
            _ = &mut first => panic!("playback should not finish immediately"),
            _ = tokio::time::sleep(Duration::from_millis(1)) => {}
        }
        let queued = worker.play(request(2.0, None, 1.0));
        tokio::pin!(queued);
        tokio::select! {
            _ = &mut queued => panic!("queued playback should wait"),
            _ = tokio::time::sleep(Duration::from_millis(1)) => {}
        }
        assert!(matches!(
            worker.play(request(3.0, None, 1.0)).await,
            Err(SophonError::ResourceLimit(_))
        ));
        assert!(first.await.is_ok());
        assert!(queued.await.is_ok());
    }
}
