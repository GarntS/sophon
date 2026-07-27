#![cfg(unix)]

use std::{
    collections::HashMap,
    fs::File,
    io::{BufRead, BufReader, Cursor},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    time::Duration,
};

use futures_util::StreamExt;
use sophon::{
    acquisition::{CANARY, ModelLifecycle},
    dbus::SophonDbus,
    domain::TranscriptionOptions,
    postprocess::{IdentityProcessor, PostProcessingPipeline},
    service::TranscriptionService,
    transport::{BUS_NAME, INTERFACE, OBJECT_PATH},
    worker::ModelWorker,
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
    let server = zbus::connection::Builder::address(bus.address.as_str())
        .unwrap()
        .name(BUS_NAME)
        .unwrap()
        .serve_at(
            OBJECT_PATH,
            SophonDbus::ready(defaults, lifecycle.clone(), service, 4_096, 60),
        )
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
    let state: String = proxy.get_property("State").await.unwrap();
    let engine: String = proxy.get_property("ActiveEngine").await.unwrap();
    let model: String = proxy.get_property("ActiveModel").await.unwrap();
    let progress: f64 = proxy.get_property("DownloadProgress").await.unwrap();
    assert_eq!(state, "Downloading");
    assert_eq!(engine, "canary");
    assert_eq!(model, CANARY.id);
    assert_eq!(progress, 0.5);

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
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 2);
    let error = results
        .into_iter()
        .find_map(Result::err)
        .expect("one concurrent request should be rejected");
    assert_error_name(error, "com.garntresearch.sophon.ResourceLimit");

    let recorded = calls.lock().unwrap().clone();
    assert_eq!(recorded.len(), 5);
    assert!(
        recorded[3..]
            .iter()
            .all(|marker| (11..=13).contains(marker))
    );
    assert_ne!(recorded[3], recorded[4]);
}
