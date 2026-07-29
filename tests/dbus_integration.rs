#![cfg(unix)]

use std::{
    collections::HashMap,
    fs::File,
    io::{BufRead, BufReader, Cursor, Read, Seek, SeekFrom},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    time::Duration,
};

use futures_util::StreamExt;
use sophon::{
    acquisition::{CANARY, ModelLifecycle, TtsLifecycle},
    config::{Config, ConfigPaths},
    dbus::SophonDbus,
    domain::{OwnedAudio, SophonError, TranscriptionOptions, TtsCapabilities, TtsRequest},
    playback::{PlaybackRequest, PlaybackWorker, SpeechPlayback},
    postprocess::{IdentityProcessor, PostProcessingPipeline},
    service::{TranscriptionService, TtsService},
    transport::{BUS_NAME, INTERFACE, OBJECT_PATH},
    tts::{TtsProvider, TtsWorker},
    worker::ModelWorker,
};
use transcribe_rs::{
    ModelCapabilities as BackendCapabilities, SpeechModel, TranscribeError, TranscribeOptions,
    TranscriptionResult,
};
use zbus::zvariant::{Fd, OwnedFd as ZbusOwnedFd, OwnedValue, Str};

struct IsolatedBus {
    child: Child,
    address: String,
}

impl IsolatedBus {
    fn start() -> Self {
        let mut command = Command::new("dbus-daemon");
        if let Some(config) = std::env::var_os("SOPHON_DBUS_SESSION_CONFIG") {
            command.arg(format!("--config-file={}", config.to_string_lossy()));
        } else {
            command.arg("--session");
        }
        let mut child = command
            .args(["--nofork", "--nopidfile", "--print-address=1"])
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("dbus-daemon must be available for D-Bus integration tests");
        let stdout = child.stdout.take().expect("dbus-daemon stdout");
        let mut reader = BufReader::new(stdout);
        let mut address = String::new();
        reader
            .read_line(&mut address)
            .expect("read isolated bus address");
        assert!(
            !address.trim().is_empty(),
            "dbus-daemon returned no address"
        );
        Self {
            child,
            address: address.trim().to_owned(),
        }
    }
}

impl Drop for IsolatedBus {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct FixtureModel {
    calls: Arc<Mutex<Vec<i16>>>,
}

impl SpeechModel for FixtureModel {
    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            name: "D-Bus fixture",
            engine_id: "fixture",
            sample_rate: 16_000,
            languages: &["en", "de"],
            supports_timestamps: false,
            supports_translation: true,
            supports_streaming: false,
        }
    }

    fn transcribe_raw(
        &mut self,
        samples: &[f32],
        _: &TranscribeOptions,
    ) -> Result<TranscriptionResult, TranscribeError> {
        let marker = (samples[0] * i16::MAX as f32).round() as i16;
        self.calls.lock().unwrap().push(marker);
        if marker == -1 {
            return Err(TranscribeError::Inference("fixture failure".into()));
        }
        std::thread::sleep(Duration::from_millis(75));
        Ok(TranscriptionResult {
            text: format!("fixture-{marker}"),
            segments: None,
        })
    }
}

struct FixtureTtsProvider {
    calls: Arc<Mutex<Vec<TtsRequest>>>,
}

impl TtsProvider for FixtureTtsProvider {
    fn provider_id(&self) -> &'static str {
        "fixture-tts"
    }

    fn model_id(&self) -> &str {
        "fixture-tts-model"
    }

    fn capabilities(&self) -> TtsCapabilities {
        TtsCapabilities {
            named_voices: true,
            voice_cloning: false,
            voice_design: false,
            speed_control: true,
        }
    }

    fn voices(&self) -> &[String] {
        static VOICES: std::sync::LazyLock<Vec<String>> =
            std::sync::LazyLock::new(|| vec!["af_heart".into(), "am_adam".into()]);
        &VOICES
    }

    fn synthesize(&mut self, request: &TtsRequest) -> Result<OwnedAudio, SophonError> {
        self.calls.lock().unwrap().push(request.clone());
        if request.text == "fail" {
            return Err(SophonError::SynthesisFailed("fixture failure".into()));
        }
        std::thread::sleep(Duration::from_millis(75));
        Ok(OwnedAudio {
            samples: vec![request.speed as f32; 240],
            sample_rate: 24_000,
        })
    }
}

struct FixturePlayback {
    calls: Arc<Mutex<Vec<PlaybackRequest>>>,
}

impl SpeechPlayback for FixturePlayback {
    fn play(&mut self, request: PlaybackRequest) -> Result<(), SophonError> {
        self.calls.lock().unwrap().push(request);
        std::thread::sleep(Duration::from_millis(25));
        Ok(())
    }
}

fn wav(marker: i16, sample_count: usize) -> Vec<u8> {
    let mut cursor = Cursor::new(Vec::new());
    let mut writer = hound::WavWriter::new(
        &mut cursor,
        hound::WavSpec {
            channels: 1,
            sample_rate: 16_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )
    .unwrap();
    for _ in 0..sample_count {
        writer.write_sample(marker).unwrap();
    }
    writer.finalize().unwrap();
    cursor.into_inner()
}

fn empty_options() -> HashMap<String, OwnedValue> {
    HashMap::new()
}

fn string_option(key: &str, value: &str) -> HashMap<String, OwnedValue> {
    HashMap::from([(
        key.to_owned(),
        OwnedValue::from(Str::from(value.to_owned())),
    )])
}

fn double_option(key: &str, value: f64) -> HashMap<String, OwnedValue> {
    HashMap::from([(key.to_owned(), OwnedValue::from(value))])
}

fn fd_option(key: &str, fd: std::os::fd::OwnedFd) -> HashMap<String, OwnedValue> {
    HashMap::from([(key.to_owned(), OwnedValue::try_from(Fd::from(fd)).unwrap())])
}

fn default_tts_config() -> sophon::config::TtsConfig {
    let root = tempfile::tempdir().unwrap();
    let paths = ConfigPaths::from_homes(root.path().join("config"), root.path().join("cache"));
    Config::load(&paths).unwrap().tts.unwrap()
}

fn assert_error_name(error: zbus::Error, expected: &str) {
    match error {
        zbus::Error::MethodError(name, _, _) => assert_eq!(name.as_str(), expected),
        other => panic!("expected D-Bus method error `{expected}`, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn isolated_session_bus_covers_the_public_contract() {
    let bus = IsolatedBus::start();
    let lifecycle = ModelLifecycle::new();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let worker = ModelWorker::new(
        Box::new(FixtureModel {
            calls: calls.clone(),
        }),
        1,
    );
    let defaults = TranscriptionOptions {
        language: Some("en".into()),
        translate: Some(false),
    };
    let service = Arc::new(TranscriptionService::new(
        lifecycle.clone(),
        worker,
        defaults.clone(),
        CANARY.capabilities.clone(),
        PostProcessingPipeline::new(vec![Box::new(IdentityProcessor)]),
        "canary".into(),
        CANARY.id.into(),
    ));
    let tts_lifecycle = TtsLifecycle::new();
    let tts_calls = Arc::new(Mutex::new(Vec::new()));
    let playback_calls = Arc::new(Mutex::new(Vec::new()));
    let mut tts_config = default_tts_config();
    tts_config.operational.queue_capacity = 2;
    let tts_worker = TtsWorker::new(
        Box::new(FixtureTtsProvider {
            calls: tts_calls.clone(),
        }),
        tts_config.operational.queue_capacity,
        tts_config.operational.max_generated_audio_seconds,
    );
    let playback = PlaybackWorker::new(
        Box::new(FixturePlayback {
            calls: playback_calls.clone(),
        }),
        tts_config.operational.queue_capacity,
    );
    let tts_service = Arc::new(TtsService::new(
        tts_lifecycle.clone(),
        tts_worker,
        playback,
        tts_config.clone(),
    ));
    let mut dbus = SophonDbus::ready(defaults, lifecycle.clone(), service, 4_096, 60);
    dbus.install_tts(tts_config.clone(), tts_lifecycle.clone(), tts_service);
    let server = zbus::connection::Builder::address(bus.address.as_str())
        .unwrap()
        .name(BUS_NAME)
        .unwrap()
        .serve_at(OBJECT_PATH, dbus)
        .unwrap()
        .build()
        .await
        .unwrap();
    let client = zbus::connection::Builder::address(bus.address.as_str())
        .unwrap()
        .build()
        .await
        .unwrap();
    let proxy = zbus::Proxy::new(&client, BUS_NAME, OBJECT_PATH, INTERFACE)
        .await
        .unwrap();

    let xml = proxy.introspect().await.unwrap();
    assert!(xml.contains("method name=\"TranscribeFile\""));
    assert!(xml.contains("method name=\"TranscribeMemfd\""));
    assert!(xml.contains("arg name=\"path\" type=\"s\" direction=\"in\""));
    assert!(xml.contains("arg name=\"fd\" type=\"h\" direction=\"in\""));
    assert!(xml.contains("arg name=\"values\" type=\"a{sv}\" direction=\"in\""));
    assert!(xml.contains("arg type=\"s\" direction=\"out\""));
    assert!(!xml.contains("method name=\"transcribe_file\""));
    assert!(xml.contains("method name=\"SpeakToFile\""));
    assert!(xml.contains("method name=\"SpeakToBuffer\""));
    assert!(xml.contains("method name=\"SpeakAloud\""));
    assert!(xml.contains("property name=\"TtsState\" type=\"s\" access=\"read\""));
    assert!(xml.contains("property name=\"AvailableVoices\" type=\"as\" access=\"read\""));
    assert!(xml.contains("property name=\"TtsCapabilities\" type=\"as\" access=\"read\""));

    let error = proxy
        .call::<_, _, (ZbusOwnedFd, u64)>("SpeakToBuffer", &("hello", empty_options()))
        .await
        .unwrap_err();
    assert_error_name(error, "com.garntresearch.sophon.NotReady");

    tts_lifecycle.loading("fixture-tts", "fixture-tts-model");
    tts_lifecycle.ready(
        vec!["af_heart".into(), "am_adam".into()],
        TtsCapabilities {
            named_voices: true,
            voice_cloning: false,
            voice_design: false,
            speed_control: true,
        },
    );
    let error = proxy
        .call::<_, _, (ZbusOwnedFd, u64)>(
            "SpeakToBuffer",
            &("hello", string_option("speed", "fast")),
        )
        .await
        .unwrap_err();
    assert_error_name(error, "com.garntresearch.sophon.InvalidTtsOptions");
    let error = proxy
        .call::<_, _, (ZbusOwnedFd, u64)>(
            "SpeakToBuffer",
            &("hello", string_option("voice", "missing")),
        )
        .await
        .unwrap_err();
    assert_error_name(error, "com.garntresearch.sophon.InvalidTtsOptions");
    let clone_file: std::os::fd::OwnedFd = tempfile::tempfile().unwrap().into();
    let error = proxy
        .call::<_, _, (ZbusOwnedFd, u64)>(
            "SpeakToBuffer",
            &("hello", fd_option("clone_audio", clone_file)),
        )
        .await
        .unwrap_err();
    assert_error_name(error, "com.garntresearch.sophon.UnsupportedCapability");
    let error = proxy
        .call::<_, _, (ZbusOwnedFd, u64)>("SpeakToBuffer", &("fail", empty_options()))
        .await
        .unwrap_err();
    assert_error_name(error, "com.garntresearch.sophon.SynthesisFailed");

    let audio = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(audio.path(), wav(7, 8)).unwrap();
    let path = audio.path().to_str().unwrap().to_owned();

    let error = proxy
        .call::<_, _, String>("TranscribeFile", &(path.clone(), empty_options()))
        .await
        .unwrap_err();
    assert_error_name(error, "com.garntresearch.sophon.NotReady");

    let error = proxy
        .call::<_, _, String>(
            "TranscribeFile",
            &(path.clone(), string_option("unknown", "value")),
        )
        .await
        .unwrap_err();
    assert_error_name(error, "com.garntresearch.sophon.InvalidOptions");

    lifecycle.loading(&CANARY);
    lifecycle.ready();
    let text: String = proxy
        .call("TranscribeFile", &(path.clone(), empty_options()))
        .await
        .unwrap();
    assert_eq!(text, "fixture-7");

    let fd: std::os::fd::OwnedFd = File::open(audio.path()).unwrap().into();
    let text: String = proxy
        .call("TranscribeMemfd", &(ZbusOwnedFd::from(fd), empty_options()))
        .await
        .unwrap();
    assert_eq!(text, "fixture-7");

    let error = proxy
        .call::<_, _, String>(
            "TranscribeFile",
            &("relative.wav".to_owned(), empty_options()),
        )
        .await
        .unwrap_err();
    assert_error_name(error, "com.garntresearch.sophon.InvalidAudio");

    let oversized = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(oversized.path(), wav(9, 4_000)).unwrap();
    let error = proxy
        .call::<_, _, String>(
            "TranscribeFile",
            &(
                oversized.path().to_str().unwrap().to_owned(),
                empty_options(),
            ),
        )
        .await
        .unwrap_err();
    assert_error_name(error, "com.garntresearch.sophon.ResourceLimit");

    let failing = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(failing.path(), wav(-1, 8)).unwrap();
    let error = proxy
        .call::<_, _, String>(
            "TranscribeFile",
            &(failing.path().to_str().unwrap().to_owned(), empty_options()),
        )
        .await
        .unwrap_err();
    assert_error_name(error, "com.garntresearch.sophon.TranscriptionFailed");

    lifecycle.failed("fixture model unavailable");
    let error = proxy
        .call::<_, _, String>("TranscribeFile", &(path.clone(), empty_options()))
        .await
        .unwrap_err();
    assert_error_name(error, "com.garntresearch.sophon.ModelUnavailable");
    let (_fd, _size): (ZbusOwnedFd, u64) = proxy
        .call(
            "SpeakToBuffer",
            &("TTS survives STT failure", empty_options()),
        )
        .await
        .unwrap();
    let tts_state_while_stt_failed: String = proxy.get_property("TtsState").await.unwrap();
    assert_eq!(tts_state_while_stt_failed, "Ready");

    let properties = zbus::fdo::PropertiesProxy::builder(&client)
        .destination(BUS_NAME)
        .unwrap()
        .path(OBJECT_PATH)
        .unwrap()
        .build()
        .await
        .unwrap();
    let mut changes = properties.receive_properties_changed().await.unwrap();
    lifecycle.loading(&CANARY);
    lifecycle.downloading(0.5);
    let interface = server
        .object_server()
        .interface::<_, SophonDbus>(OBJECT_PATH)
        .await
        .unwrap();
    SophonDbus::emit_lifecycle_changed(&interface)
        .await
        .unwrap();

    let changed = tokio::time::timeout(Duration::from_secs(2), changes.next())
        .await
        .expect("PropertiesChanged signal timed out")
        .expect("PropertiesChanged stream ended");
    assert_eq!(changed.args().unwrap().interface_name(), INTERFACE);
    let stt_interface_name = zbus::names::InterfaceName::try_from(INTERFACE).unwrap();
    let state = String::try_from(
        properties
            .get(stt_interface_name.clone(), "State")
            .await
            .unwrap(),
    )
    .unwrap();
    let engine = String::try_from(
        properties
            .get(stt_interface_name.clone(), "ActiveEngine")
            .await
            .unwrap(),
    )
    .unwrap();
    let model = String::try_from(
        properties
            .get(stt_interface_name.clone(), "ActiveModel")
            .await
            .unwrap(),
    )
    .unwrap();
    let progress = f64::try_from(
        properties
            .get(stt_interface_name, "DownloadProgress")
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(state, "Downloading");
    assert_eq!(engine, "canary");
    assert_eq!(model, CANARY.id);
    assert_eq!(progress, 0.5);

    let mut tts_changes = properties.receive_properties_changed().await.unwrap();
    tts_lifecycle.downloading(0.25);
    SophonDbus::emit_tts_lifecycle_changed(&interface)
        .await
        .unwrap();
    let changed = tokio::time::timeout(Duration::from_secs(2), tts_changes.next())
        .await
        .expect("TTS PropertiesChanged signal timed out")
        .expect("TTS PropertiesChanged stream ended");
    assert_eq!(changed.args().unwrap().interface_name(), INTERFACE);
    let interface_name = zbus::names::InterfaceName::try_from(INTERFACE).unwrap();
    let tts_state = String::try_from(
        properties
            .get(interface_name.clone(), "TtsState")
            .await
            .unwrap(),
    )
    .unwrap();
    let tts_provider = String::try_from(
        properties
            .get(interface_name.clone(), "ActiveTtsProvider")
            .await
            .unwrap(),
    )
    .unwrap();
    let tts_model = String::try_from(
        properties
            .get(interface_name.clone(), "ActiveTtsModel")
            .await
            .unwrap(),
    )
    .unwrap();
    let tts_progress = f64::try_from(
        properties
            .get(interface_name, "TtsDownloadProgress")
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(tts_state, "Downloading");
    assert_eq!(tts_provider, "fixture-tts");
    assert_eq!(tts_model, "fixture-tts-model");
    assert_eq!(tts_progress, 0.25);
    tts_lifecycle.ready(
        vec!["af_heart".into(), "am_adam".into()],
        TtsCapabilities {
            named_voices: true,
            voice_cloning: false,
            voice_design: false,
            speed_control: true,
        },
    );
    let capabilities = Vec::<String>::try_from(
        properties
            .get(
                zbus::names::InterfaceName::try_from(INTERFACE).unwrap(),
                "TtsCapabilities",
            )
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(capabilities, ["named-voices", "speed-control"]);

    for (model, voices, capabilities, expected_capability) in [
        (
            "qwen3-tts-0.6b-base-q8_0",
            Vec::new(),
            TtsCapabilities {
                named_voices: false,
                voice_cloning: true,
                voice_design: false,
                speed_control: false,
            },
            "voice-cloning",
        ),
        (
            "qwen3-tts-0.6b-custom-voice-q8_0",
            vec!["vivian".into(), "ryan".into()],
            TtsCapabilities {
                named_voices: true,
                voice_cloning: false,
                voice_design: false,
                speed_control: false,
            },
            "named-voices",
        ),
        (
            "qwen3-tts-1.7b-voice-design-q8_0",
            Vec::new(),
            TtsCapabilities {
                named_voices: false,
                voice_cloning: false,
                voice_design: true,
                speed_control: false,
            },
            "voice-design",
        ),
    ] {
        tts_lifecycle.loading("qwentts-cpp", model);
        tts_lifecycle.downloading(0.5);
        tts_lifecycle.ready(voices.clone(), capabilities);
        let interface_name = zbus::names::InterfaceName::try_from(INTERFACE).unwrap();
        let active_provider = String::try_from(
            properties
                .get(interface_name.clone(), "ActiveTtsProvider")
                .await
                .unwrap(),
        )
        .unwrap();
        let active_model = String::try_from(
            properties
                .get(interface_name.clone(), "ActiveTtsModel")
                .await
                .unwrap(),
        )
        .unwrap();
        let available_voices = Vec::<String>::try_from(
            properties
                .get(interface_name.clone(), "AvailableVoices")
                .await
                .unwrap(),
        )
        .unwrap();
        let advertised = Vec::<String>::try_from(
            properties
                .get(interface_name.clone(), "TtsCapabilities")
                .await
                .unwrap(),
        )
        .unwrap();
        let progress = f64::try_from(
            properties
                .get(interface_name, "TtsDownloadProgress")
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(active_provider, "qwentts-cpp");
        assert_eq!(active_model, model);
        assert_eq!(available_voices, voices);
        assert_eq!(advertised, [expected_capability]);
        assert_eq!(progress, 0.5);
    }
    tts_lifecycle.failed("isolated Qwen initialization failure");
    let interface_name = zbus::names::InterfaceName::try_from(INTERFACE).unwrap();
    assert_eq!(
        String::try_from(
            properties
                .get(interface_name.clone(), "TtsState")
                .await
                .unwrap()
        )
        .unwrap(),
        "Failed"
    );
    assert_eq!(
        String::try_from(properties.get(interface_name, "State").await.unwrap()).unwrap(),
        "Downloading"
    );
    tts_lifecycle.ready(
        vec!["af_heart".into(), "am_adam".into()],
        TtsCapabilities {
            named_voices: true,
            voice_cloning: false,
            voice_design: false,
            speed_control: true,
        },
    );

    lifecycle.ready();
    let first = tempfile::NamedTempFile::new().unwrap();
    let second = tempfile::NamedTempFile::new().unwrap();
    let third = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(first.path(), wav(11, 8)).unwrap();
    std::fs::write(second.path(), wav(12, 8)).unwrap();
    std::fs::write(third.path(), wav(13, 8)).unwrap();
    let request = |request_path: String| {
        let proxy = proxy.clone();
        async move {
            proxy
                .call::<_, _, String>("TranscribeFile", &(request_path, empty_options()))
                .await
        }
    };
    let (one, two, three) = tokio::join!(
        request(first.path().to_str().unwrap().to_owned()),
        request(second.path().to_str().unwrap().to_owned()),
        request(third.path().to_str().unwrap().to_owned()),
    );
    let results = [one, two, three];
    assert!((1..=2).contains(&results.iter().filter(|result| result.is_ok()).count()));
    let error = results
        .into_iter()
        .find_map(Result::err)
        .expect("one concurrent request should be rejected");
    assert_error_name(error, "com.garntresearch.sophon.ResourceLimit");

    let output_dir = tempfile::tempdir().unwrap();
    let output_path = output_dir.path().join("speech.wav");
    let size: u64 = proxy
        .call(
            "SpeakToFile",
            &(
                "named voice",
                output_path.to_str().unwrap(),
                string_option("voice", "am_adam"),
            ),
        )
        .await
        .unwrap();
    assert_eq!(size, std::fs::metadata(&output_path).unwrap().len());
    let reader = hound::WavReader::open(&output_path).unwrap();
    assert_eq!(reader.spec().channels, 1);
    assert_eq!(reader.spec().sample_rate, 24_000);
    assert_eq!(reader.spec().sample_format, hound::SampleFormat::Float);
    let error = proxy
        .call::<_, _, u64>(
            "SpeakToFile",
            &(
                "must not replace",
                output_path.to_str().unwrap(),
                empty_options(),
            ),
        )
        .await
        .unwrap_err();
    assert_error_name(error, "com.garntresearch.sophon.OutputExists");

    let (buffer_fd, buffer_size): (ZbusOwnedFd, u64) = proxy
        .call("SpeakToBuffer", &("buffer", double_option("speed", 1.5)))
        .await
        .unwrap();
    let fd: std::os::fd::OwnedFd = buffer_fd.into();
    let mut file = File::from(fd);
    assert_eq!(file.stream_position().unwrap(), 0);
    let mut buffer_bytes = Vec::new();
    file.read_to_end(&mut buffer_bytes).unwrap();
    assert_eq!(buffer_size, buffer_bytes.len() as u64);
    let memfd = memfd::Memfd::try_from_file(file).unwrap();
    let seals = memfd.seals().unwrap();
    assert!(seals.contains(&memfd::FileSeal::SealWrite));
    assert!(seals.contains(&memfd::FileSeal::SealGrow));
    assert!(seals.contains(&memfd::FileSeal::SealShrink));
    assert!(seals.contains(&memfd::FileSeal::SealSeal));
    let mut client_file = memfd.into_file();
    client_file.seek(SeekFrom::Start(0)).unwrap();
    let reader = hound::WavReader::new(client_file).unwrap();
    assert_eq!(reader.spec().sample_rate, 24_000);

    let started = std::time::Instant::now();
    let aloud_one_args = ("aloud one", empty_options());
    let aloud_two_args = ("aloud two", empty_options());
    let (aloud_one, aloud_two) = tokio::join!(
        proxy.call::<_, _, ()>("SpeakAloud", &aloud_one_args),
        proxy.call::<_, _, ()>("SpeakAloud", &aloud_two_args),
    );
    assert!(aloud_one.is_ok());
    assert!(aloud_two.is_ok());
    assert!(started.elapsed() >= Duration::from_millis(50));
    assert_eq!(playback_calls.lock().unwrap().len(), 2);

    let request_tts = |text: &'static str| {
        let proxy = proxy.clone();
        async move {
            proxy
                .call::<_, _, (ZbusOwnedFd, u64)>("SpeakToBuffer", &(text, empty_options()))
                .await
        }
    };
    let (one, two, three, four) = tokio::join!(
        request_tts("queue one"),
        request_tts("queue two"),
        request_tts("queue three"),
        request_tts("queue four"),
    );
    let tts_results = [one, two, three, four];
    assert!((1..=3).contains(&tts_results.iter().filter(|result| result.is_ok()).count()));
    let error = tts_results
        .into_iter()
        .find_map(Result::err)
        .expect("one concurrent TTS request should be rejected");
    assert_error_name(error, "com.garntresearch.sophon.ResourceLimit");

    let recorded = calls.lock().unwrap().clone();
    assert!((4..=5).contains(&recorded.len()));
    assert!(
        recorded[3..]
            .iter()
            .all(|marker| (11..=13).contains(marker))
    );
    if recorded.len() == 5 {
        assert_ne!(recorded[3], recorded[4]);
    }

    tts_lifecycle.failed("fixture TTS initialization failed");
    SophonDbus::emit_tts_lifecycle_changed(&interface)
        .await
        .unwrap();
    lifecycle.ready();
    let text: String = proxy
        .call("TranscribeFile", &(path, empty_options()))
        .await
        .unwrap();
    assert_eq!(text, "fixture-7");
    let interface_name = zbus::names::InterfaceName::try_from(INTERFACE).unwrap();
    let tts_state_after_stt_use =
        String::try_from(properties.get(interface_name, "TtsState").await.unwrap()).unwrap();
    assert_eq!(tts_state_after_stt_use, "Failed");
}
