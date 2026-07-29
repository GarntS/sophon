#![cfg(any(feature = "qwen-cpu", feature = "qwen-cuda", feature = "qwen-vulkan"))]

use std::{
    env,
    path::{Path, PathBuf},
};

use sophon::{
    acquisition::{
        QWEN_06B_BASE, QWEN_06B_CUSTOM_VOICE, QWEN_17B_VOICE_DESIGN, ResolvedQwenModel,
        TtsModelDefinition,
    },
    config::QwenSamplingConfig,
    domain::{OwnedAudio, TtsRequest, VoiceIntent},
    tts::{
        QwenEngineAdapter, QwenTtsBaseProvider, QwenTtsCustomVoiceProvider,
        QwenTtsVoiceDesignProvider, TtsProvider,
    },
};

fn required_path(name: &str) -> PathBuf {
    env::var_os(name)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("set {name} to run the ignored Qwen real-model smoke test"))
}

fn resolved(
    definition: &'static TtsModelDefinition,
    talker_env: &str,
    codec: &Path,
) -> ResolvedQwenModel {
    ResolvedQwenModel {
        definition,
        metadata: definition.qwen.expect("Qwen metadata"),
        talker_path: required_path(talker_env),
        codec_path: codec.to_path_buf(),
    }
}

fn request(voice: VoiceIntent) -> TtsRequest {
    TtsRequest {
        text: "The quick brown fox speaks clearly.".into(),
        language: Some("en-US".into()),
        speed: 1.0,
        voice,
    }
}

fn assert_audio(audio: OwnedAudio) {
    assert_eq!(audio.sample_rate, 24_000);
    assert!(!audio.samples.is_empty());
    assert!(audio.samples.iter().all(|sample| sample.is_finite()));
}

#[test]
#[ignore = "requires multi-gigabyte curated Qwen GGUF files; opt in explicitly"]
fn real_qwen_modes_produce_finite_nonempty_24khz_audio() {
    let codec = required_path("SOPHON_QWEN_CODEC");
    let sampling = QwenSamplingConfig {
        seed: Some(42),
        ..QwenSamplingConfig::default()
    };

    let base_model = resolved(&QWEN_06B_BASE, "SOPHON_QWEN_BASE_TALKER", &codec);
    let base_engine = QwenEngineAdapter::load(&base_model, &sampling, 30).unwrap();
    let mut base = QwenTtsBaseProvider::new(base_engine, base_model.definition.id, 16 * 1024);
    assert_audio(base.synthesize(&request(VoiceIntent::Default)).unwrap());
    let reference = OwnedAudio {
        samples: (0..24_000 * 3)
            .map(|index| {
                let phase = index as f32 * 220.0 * std::f32::consts::TAU / 24_000.0;
                phase.sin() * 0.1
            })
            .collect(),
        sample_rate: 24_000,
    };
    assert_audio(
        base.synthesize(&request(VoiceIntent::Clone {
            reference,
            transcript: Some("A steady reference tone.".into()),
        }))
        .unwrap(),
    );

    let custom_model = resolved(
        &QWEN_06B_CUSTOM_VOICE,
        "SOPHON_QWEN_CUSTOM_VOICE_TALKER",
        &codec,
    );
    let custom_engine = QwenEngineAdapter::load(&custom_model, &sampling, 30).unwrap();
    let mut custom = QwenTtsCustomVoiceProvider::new(
        custom_engine,
        custom_model.definition.id,
        "vivian",
        16 * 1024,
    )
    .unwrap();
    assert_audio(
        custom
            .synthesize(&request(VoiceIntent::Named("vivian".into())))
            .unwrap(),
    );

    let design_model = resolved(
        &QWEN_17B_VOICE_DESIGN,
        "SOPHON_QWEN_VOICE_DESIGN_TALKER",
        &codec,
    );
    let design_engine = QwenEngineAdapter::load(&design_model, &sampling, 30).unwrap();
    let mut design = QwenTtsVoiceDesignProvider::new(
        design_engine,
        design_model.definition.id,
        "A warm, clear, natural adult voice with moderate pitch and pace.",
        16 * 1024,
    );
    assert_audio(
        design
            .synthesize(&request(VoiceIntent::Design(
                "A bright, precise narrator with an even pace.".into(),
            )))
            .unwrap(),
    );
}
