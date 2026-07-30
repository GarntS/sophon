#![cfg(unix)]

use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Cursor},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    time::Duration,
};

use sophon::{
    audio::OwnedAudio,
    config::{Config, ConfigPaths},
    dbus::{
        SophonDbus,
        transport::{BUS_NAME, INTERFACE, OBJECT_PATH},
    },
    error::SophonError,
    stt::{STTService, STTWorker, TranscriptionOptions},
    tts::{
        TtsCapabilities, TtsProvider, TtsRequest, TtsService, TtsWorker,
        playback::{PlaybackRequest, PlaybackWorker, SpeechPlayback},
    },
};
use transcribe_rs::{
    ModelCapabilities as BackendCapabilities, SpeechModel, TranscribeError, TranscribeOptions,
    TranscriptionResult,
};
use zbus::zvariant::{OwnedFd as ZbusOwnedFd, OwnedValue, Str};

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
async fn isolated_session_bus_covers_provider_owned_states_and_language_only_options() {
    use sophon::{
        model_registry::{ModelCatalog, ModelRegistry},
        provider_runtime::{SttProviderHandle, TtsProviderHandle},
    };

    let bus = IsolatedBus::start();
    let cache = tempfile::tempdir().unwrap();
    let registry = Arc::new(ModelRegistry::new(
        ModelCatalog::from_yaml(include_str!("../model_registry.yaml")).unwrap(),
        cache.path().into(),
        reqwest::Client::new(),
    ));
    let stt_handle =
        SttProviderHandle::new(Arc::clone(&registry), "fixture-stt", "fixture-stt-model");
    let tts_handle = TtsProviderHandle::new(registry, "fixture-tts", "fixture-tts-model");

    let calls = Arc::new(Mutex::new(Vec::new()));
    let worker = STTWorker::new(
        Box::new(FixtureModel {
            calls: calls.clone(),
        }),
        1,
    );
    let defaults = TranscriptionOptions {
        language: Some("en".into()),
    };
    let service = Arc::new(STTService::new(
        worker,
        defaults.clone(),
        vec!["en".into(), "de".into()],
    ));

    let tts_calls = Arc::new(Mutex::new(Vec::new()));
    let playback_calls = Arc::new(Mutex::new(Vec::new()));
    let mut tts_config = default_tts_config();
    tts_config.operational.queue_capacity = 2;
    let tts_worker = TtsWorker::new(
        Box::new(FixtureTtsProvider {
            calls: tts_calls.clone(),
        }),
        2,
        tts_config.operational.max_generated_audio_seconds,
    );
    let playback = PlaybackWorker::new(
        Box::new(FixturePlayback {
            calls: playback_calls,
        }),
        2,
    );
    let tts_service = Arc::new(TtsService::new(
        tts_worker,
        playback,
        tts_config.clone(),
        vec!["en".into()],
    ));

    let mut dbus = SophonDbus::ready(defaults, stt_handle.clone(), tts_handle.clone(), 4_096, 60);
    dbus.install_tts(tts_config.clone());
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

    let audio = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(audio.path(), wav(7, 8)).unwrap();
    let path = audio.path().to_str().unwrap().to_owned();
    let error = proxy
        .call::<_, _, String>("TranscribeFile", &(path.clone(), empty_options()))
        .await
        .unwrap_err();
    assert_error_name(error, "com.garntresearch.sophon.NotReady");

    stt_handle.loading();
    stt_handle.ready(service.clone());
    let text: String = proxy
        .call("TranscribeFile", &(path.clone(), empty_options()))
        .await
        .unwrap();
    assert_eq!(text, "fixture-7");
    let text: String = proxy
        .call(
            "TranscribeFile",
            &(path.clone(), string_option("language", "de")),
        )
        .await
        .unwrap();
    assert_eq!(text, "fixture-7");
    let error = proxy
        .call::<_, _, String>(
            "TranscribeFile",
            &(path.clone(), string_option("translate", "true")),
        )
        .await
        .unwrap_err();
    assert_error_name(error, "com.garntresearch.sophon.InvalidOptions");

    let capabilities = TtsCapabilities {
        named_voices: true,
        voice_cloning: false,
        voice_design: false,
        speed_control: true,
    };
    tts_handle.loading();
    tts_handle.ready(
        tts_service,
        vec!["af_heart".into(), "am_adam".into()],
        capabilities,
    );
    let (_fd, _size): (ZbusOwnedFd, u64) = proxy
        .call(
            "SpeakToBuffer",
            &("request override", string_option("voice", "am_adam")),
        )
        .await
        .unwrap();
    let (_fd, _size): (ZbusOwnedFd, u64) = proxy
        .call("SpeakToBuffer", &("configured default", empty_options()))
        .await
        .unwrap();
    {
        let requests = tts_calls.lock().unwrap();
        assert!(
            matches!(&requests[0].voice, sophon::tts::VoiceIntent::Named(voice) if voice == "am_adam")
        );
        assert!(matches!(
            &requests[1].voice,
            sophon::tts::VoiceIntent::Default
        ));
    }

    let interface = server
        .object_server()
        .interface::<_, SophonDbus>(OBJECT_PATH)
        .await
        .unwrap();
    SophonDbus::emit_lifecycle_changed(&interface)
        .await
        .unwrap();
    SophonDbus::emit_tts_lifecycle_changed(&interface)
        .await
        .unwrap();
    assert_eq!(
        proxy.get_property::<String>("State").await.unwrap(),
        "Ready"
    );
    assert_eq!(
        proxy
            .get_property::<String>("ActiveProvider")
            .await
            .unwrap(),
        "fixture-stt"
    );
    assert_eq!(
        proxy.get_property::<String>("ActiveModel").await.unwrap(),
        "fixture-stt-model"
    );
    assert_eq!(
        proxy.get_property::<String>("TtsState").await.unwrap(),
        "Ready"
    );
    assert_eq!(
        proxy
            .get_property::<String>("ActiveTtsProvider")
            .await
            .unwrap(),
        "fixture-tts"
    );
    assert_eq!(
        proxy
            .get_property::<String>("ActiveTtsModel")
            .await
            .unwrap(),
        "fixture-tts-model"
    );

    tts_handle.failed("fixture TTS initialization failed");
    SophonDbus::emit_tts_lifecycle_changed(&interface)
        .await
        .unwrap();
    let fresh_proxy = zbus::Proxy::new(&client, BUS_NAME, OBJECT_PATH, INTERFACE)
        .await
        .unwrap();
    assert_eq!(
        fresh_proxy
            .get_property::<String>("TtsState")
            .await
            .unwrap(),
        "Failed"
    );
    assert_eq!(
        fresh_proxy.get_property::<String>("State").await.unwrap(),
        "Ready"
    );
    let text: String = proxy
        .call("TranscribeFile", &(path, empty_options()))
        .await
        .unwrap();
    assert_eq!(text, "fixture-7");
}
