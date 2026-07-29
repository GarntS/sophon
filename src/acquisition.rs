//! Curated Hugging Face model registry, cache validation, and verified acquisition.

use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};

use fs2::FileExt;
use futures_util::StreamExt;
use sha2::{Digest, Sha256};

use crate::{
    config::{Engine, Quantization},
    domain::{SophonError, TtsCapabilities, TtsState},
};

#[derive(Debug, Clone, PartialEq)]
pub struct LifecycleSnapshot {
    pub state: crate::domain::ModelState,
    pub active_engine: Option<Engine>,
    pub active_model: Option<String>,
    pub download_progress: f32,
    pub last_error: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ModelLifecycle(Arc<RwLock<LifecycleSnapshot>>);

impl ModelLifecycle {
    pub fn new() -> Self {
        Self(Arc::new(RwLock::new(LifecycleSnapshot {
            state: crate::domain::ModelState::Initializing,
            active_engine: None,
            active_model: None,
            download_progress: 0.0,
            last_error: None,
        })))
    }
    pub fn snapshot(&self) -> LifecycleSnapshot {
        self.0.read().expect("lifecycle lock poisoned").clone()
    }
    pub fn downloading(&self, progress: f32) {
        let mut s = self.0.write().expect("lifecycle lock poisoned");
        s.state = crate::domain::ModelState::Downloading { progress };
        s.download_progress = progress.clamp(0.0, 1.0);
    }
    pub fn loading(&self, model: &ModelDefinition) {
        let mut s = self.0.write().expect("lifecycle lock poisoned");
        s.state = crate::domain::ModelState::Loading;
        s.active_engine = Some(model.engine);
        s.active_model = Some(model.id.into());
    }
    pub fn ready(&self) {
        self.0.write().expect("lifecycle lock poisoned").state = crate::domain::ModelState::Ready;
    }
    pub fn failed(&self, error: impl Into<String>) {
        let mut s = self.0.write().expect("lifecycle lock poisoned");
        let error = error.into();
        s.state = crate::domain::ModelState::Failed {
            message: error.clone(),
        };
        s.last_error = Some(error);
    }
}

impl Default for ModelLifecycle {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TtsLifecycleSnapshot {
    pub state: TtsState,
    pub active_provider: Option<String>,
    pub active_model: Option<String>,
    pub download_progress: f32,
    pub last_error: Option<String>,
    pub available_voices: Vec<String>,
    pub capabilities: TtsCapabilities,
}

#[derive(Clone, Debug)]
pub struct TtsLifecycle(Arc<RwLock<TtsLifecycleSnapshot>>);

impl TtsLifecycle {
    pub fn new() -> Self {
        Self(Arc::new(RwLock::new(TtsLifecycleSnapshot {
            state: TtsState::Initializing,
            active_provider: None,
            active_model: None,
            download_progress: 0.0,
            last_error: None,
            available_voices: Vec::new(),
            capabilities: TtsCapabilities {
                named_voices: false,
                voice_cloning: false,
                voice_design: false,
                speed_control: false,
            },
        })))
    }

    pub fn snapshot(&self) -> TtsLifecycleSnapshot {
        self.0.read().expect("TTS lifecycle lock poisoned").clone()
    }

    pub fn downloading(&self, progress: f32) {
        let mut snapshot = self.0.write().expect("TTS lifecycle lock poisoned");
        snapshot.state = TtsState::Downloading { progress };
        snapshot.download_progress = progress.clamp(0.0, 1.0);
    }

    pub fn loading(&self, provider: impl Into<String>, model: impl Into<String>) {
        let mut snapshot = self.0.write().expect("TTS lifecycle lock poisoned");
        snapshot.state = TtsState::Loading;
        snapshot.active_provider = Some(provider.into());
        snapshot.active_model = Some(model.into());
    }

    pub fn ready(&self, voices: Vec<String>, capabilities: TtsCapabilities) {
        let mut snapshot = self.0.write().expect("TTS lifecycle lock poisoned");
        snapshot.state = TtsState::Ready;
        snapshot.available_voices = voices;
        snapshot.capabilities = capabilities;
        snapshot.last_error = None;
    }

    pub fn failed(&self, error: impl Into<String>) {
        let mut snapshot = self.0.write().expect("TTS lifecycle lock poisoned");
        let error = error.into();
        snapshot.state = TtsState::Failed {
            message: error.clone(),
        };
        snapshot.last_error = Some(error);
    }
}

impl Default for TtsLifecycle {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ArtifactIdentity {
    pub sha256: &'static str,
    pub expected_size: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelFile {
    pub relative_path: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
    pub expected_size: u64,
}

impl ModelFile {
    pub const fn identity(&self) -> ArtifactIdentity {
        ArtifactIdentity {
            sha256: self.sha256,
            expected_size: self.expected_size,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelCapabilities {
    pub languages: &'static [&'static str],
    pub translation_to_english: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelDefinition {
    pub id: &'static str,
    pub engine: Engine,
    pub revision: &'static str,
    pub quantization: Quantization,
    pub files: &'static [ModelFile],
    pub capabilities: ModelCapabilities,
}

/// Provider-neutral manifest used by both STT and TTS acquisition registries.
pub trait ArtifactManifest: Sync {
    fn id(&self) -> &'static str;
    fn revision(&self) -> &'static str;
    fn files(&self) -> &'static [ModelFile];
}

impl ArtifactManifest for ModelDefinition {
    fn id(&self) -> &'static str {
        self.id
    }

    fn revision(&self) -> &'static str {
        self.revision
    }

    fn files(&self) -> &'static [ModelFile] {
        self.files
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QwenTtsMode {
    Base,
    CustomVoice,
    VoiceDesign,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QwenModelSize {
    Billion06,
    Billion17,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QwenModelMetadata {
    pub mode: QwenTtsMode,
    pub size: QwenModelSize,
    pub quantization: &'static str,
    pub talker: ModelFile,
    pub codec: ModelFile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TtsModelDefinition {
    pub id: &'static str,
    pub provider: &'static str,
    pub revision: &'static str,
    pub files: &'static [ModelFile],
    pub qwen: Option<QwenModelMetadata>,
}

impl ArtifactManifest for TtsModelDefinition {
    fn id(&self) -> &'static str {
        self.id
    }

    fn revision(&self) -> &'static str {
        self.revision
    }

    fn files(&self) -> &'static [ModelFile] {
        self.files
    }
}

const KOKORO_FILES: &[ModelFile] = &[
    ModelFile {
        relative_path: "kokoro-v1.0.int8.onnx",
        url: "https://github.com/thewh1teagle/kokoro-onnx/releases/download/model-files-v1.0/kokoro-v1.0.int8.onnx",
        sha256: "6e742170d309016e5891a994e1ce1559c702a2ccd0075e67ef7157974f6406cb",
        expected_size: 92_361_271,
    },
    ModelFile {
        relative_path: "voices-v1.0.bin",
        url: "https://github.com/thewh1teagle/kokoro-onnx/releases/download/model-files-v1.0/voices-v1.0.bin",
        sha256: "bca610b8308e8d99f32e6fe4197e7ec01679264efed0cac9140fe9c29f1fbf7d",
        expected_size: 28_214_398,
    },
];

pub const KOKORO: TtsModelDefinition = TtsModelDefinition {
    id: "kokoro-v1.0-int8",
    provider: "tts-rs",
    revision: "model-files-v1.0",
    files: KOKORO_FILES,
    qwen: None,
};

pub const QWEN_REVISION: &str = "e0f336a048a3de02b29b8ad92969217d9ecffe3e";
pub const QWEN_TALKER_06B_BASE_Q8_0: ModelFile = ModelFile {
    relative_path: "qwen-talker-0.6b-base-Q8_0.gguf",
    url: concat!(
        "https://huggingface.co/Serveurperso/Qwen3-TTS-GGUF/resolve/",
        "e0f336a048a3de02b29b8ad92969217d9ecffe3e/qwen-talker-0.6b-base-Q8_0.gguf"
    ),
    sha256: "d54dbaf10591421fa764ed630d764efa717ae40cd959bd48c66d4eb1af226426",
    expected_size: 992_615_488,
};
pub const QWEN_TALKER_06B_CUSTOM_VOICE_Q8_0: ModelFile = ModelFile {
    relative_path: "qwen-talker-0.6b-customvoice-Q8_0.gguf",
    url: concat!(
        "https://huggingface.co/Serveurperso/Qwen3-TTS-GGUF/resolve/",
        "e0f336a048a3de02b29b8ad92969217d9ecffe3e/qwen-talker-0.6b-customvoice-Q8_0.gguf"
    ),
    sha256: "4eb38675c736ed6ac72012846ac8d6ef80e5af8bc05726870f0b3a6569588519",
    expected_size: 968_588_544,
};
pub const QWEN_TALKER_17B_BASE_Q8_0: ModelFile = ModelFile {
    relative_path: "qwen-talker-1.7b-base-Q8_0.gguf",
    url: concat!(
        "https://huggingface.co/Serveurperso/Qwen3-TTS-GGUF/resolve/",
        "e0f336a048a3de02b29b8ad92969217d9ecffe3e/qwen-talker-1.7b-base-Q8_0.gguf"
    ),
    sha256: "4b9a33a236908dd9435a42f7a396e38038329d053b704342a6413c08544c4fda",
    expected_size: 2_079_448_256,
};
pub const QWEN_TALKER_17B_CUSTOM_VOICE_Q8_0: ModelFile = ModelFile {
    relative_path: "qwen-talker-1.7b-customvoice-Q8_0.gguf",
    url: concat!(
        "https://huggingface.co/Serveurperso/Qwen3-TTS-GGUF/resolve/",
        "e0f336a048a3de02b29b8ad92969217d9ecffe3e/qwen-talker-1.7b-customvoice-Q8_0.gguf"
    ),
    sha256: "cab2cff67a0a557310febe558dc83076b28ed790e491867eb2751759f4cd89fa",
    expected_size: 2_042_834_304,
};
pub const QWEN_TALKER_17B_VOICE_DESIGN_Q8_0: ModelFile = ModelFile {
    relative_path: "qwen-talker-1.7b-voicedesign-Q8_0.gguf",
    url: concat!(
        "https://huggingface.co/Serveurperso/Qwen3-TTS-GGUF/resolve/",
        "e0f336a048a3de02b29b8ad92969217d9ecffe3e/qwen-talker-1.7b-voicedesign-Q8_0.gguf"
    ),
    sha256: "575610ab1ddcca4dca6bd9a64bcd859d93bbad8764f9cab24e1dbc0c51f62276",
    expected_size: 2_042_833_824,
};
pub const QWEN_CODEC_Q8_0: ModelFile = ModelFile {
    relative_path: "qwen-tokenizer-12hz-Q8_0.gguf",
    url: concat!(
        "https://huggingface.co/Serveurperso/Qwen3-TTS-GGUF/resolve/",
        "e0f336a048a3de02b29b8ad92969217d9ecffe3e/qwen-tokenizer-12hz-Q8_0.gguf"
    ),
    sha256: "1883beeed99348fc35e23dd225e9082f93f6f8c109330a33d935baa8acdbfd94",
    expected_size: 291_150_624,
};

pub const QWEN_Q8_0_ARTIFACTS: &[ModelFile] = &[
    QWEN_TALKER_06B_BASE_Q8_0,
    QWEN_TALKER_06B_CUSTOM_VOICE_Q8_0,
    QWEN_TALKER_17B_BASE_Q8_0,
    QWEN_TALKER_17B_CUSTOM_VOICE_Q8_0,
    QWEN_TALKER_17B_VOICE_DESIGN_Q8_0,
    QWEN_CODEC_Q8_0,
];

const QWEN_06B_BASE_FILES: &[ModelFile] = &[QWEN_TALKER_06B_BASE_Q8_0, QWEN_CODEC_Q8_0];
const QWEN_17B_BASE_FILES: &[ModelFile] = &[QWEN_TALKER_17B_BASE_Q8_0, QWEN_CODEC_Q8_0];
const QWEN_06B_CUSTOM_VOICE_FILES: &[ModelFile] =
    &[QWEN_TALKER_06B_CUSTOM_VOICE_Q8_0, QWEN_CODEC_Q8_0];
const QWEN_17B_CUSTOM_VOICE_FILES: &[ModelFile] =
    &[QWEN_TALKER_17B_CUSTOM_VOICE_Q8_0, QWEN_CODEC_Q8_0];
const QWEN_17B_VOICE_DESIGN_FILES: &[ModelFile] =
    &[QWEN_TALKER_17B_VOICE_DESIGN_Q8_0, QWEN_CODEC_Q8_0];

pub const QWEN_06B_BASE: TtsModelDefinition = TtsModelDefinition {
    id: "qwen3-tts-0.6b-base-q8_0",
    provider: "qwentts-cpp",
    revision: QWEN_REVISION,
    files: QWEN_06B_BASE_FILES,
    qwen: Some(QwenModelMetadata {
        mode: QwenTtsMode::Base,
        size: QwenModelSize::Billion06,
        quantization: "Q8_0",
        talker: QWEN_TALKER_06B_BASE_Q8_0,
        codec: QWEN_CODEC_Q8_0,
    }),
};
pub const QWEN_17B_BASE: TtsModelDefinition = TtsModelDefinition {
    id: "qwen3-tts-1.7b-base-q8_0",
    provider: "qwentts-cpp",
    revision: QWEN_REVISION,
    files: QWEN_17B_BASE_FILES,
    qwen: Some(QwenModelMetadata {
        mode: QwenTtsMode::Base,
        size: QwenModelSize::Billion17,
        quantization: "Q8_0",
        talker: QWEN_TALKER_17B_BASE_Q8_0,
        codec: QWEN_CODEC_Q8_0,
    }),
};
pub const QWEN_06B_CUSTOM_VOICE: TtsModelDefinition = TtsModelDefinition {
    id: "qwen3-tts-0.6b-custom-voice-q8_0",
    provider: "qwentts-cpp",
    revision: QWEN_REVISION,
    files: QWEN_06B_CUSTOM_VOICE_FILES,
    qwen: Some(QwenModelMetadata {
        mode: QwenTtsMode::CustomVoice,
        size: QwenModelSize::Billion06,
        quantization: "Q8_0",
        talker: QWEN_TALKER_06B_CUSTOM_VOICE_Q8_0,
        codec: QWEN_CODEC_Q8_0,
    }),
};
pub const QWEN_17B_CUSTOM_VOICE: TtsModelDefinition = TtsModelDefinition {
    id: "qwen3-tts-1.7b-custom-voice-q8_0",
    provider: "qwentts-cpp",
    revision: QWEN_REVISION,
    files: QWEN_17B_CUSTOM_VOICE_FILES,
    qwen: Some(QwenModelMetadata {
        mode: QwenTtsMode::CustomVoice,
        size: QwenModelSize::Billion17,
        quantization: "Q8_0",
        talker: QWEN_TALKER_17B_CUSTOM_VOICE_Q8_0,
        codec: QWEN_CODEC_Q8_0,
    }),
};
pub const QWEN_17B_VOICE_DESIGN: TtsModelDefinition = TtsModelDefinition {
    id: "qwen3-tts-1.7b-voice-design-q8_0",
    provider: "qwentts-cpp",
    revision: QWEN_REVISION,
    files: QWEN_17B_VOICE_DESIGN_FILES,
    qwen: Some(QwenModelMetadata {
        mode: QwenTtsMode::VoiceDesign,
        size: QwenModelSize::Billion17,
        quantization: "Q8_0",
        talker: QWEN_TALKER_17B_VOICE_DESIGN_Q8_0,
        codec: QWEN_CODEC_Q8_0,
    }),
};

pub const QWEN_MODELS: &[TtsModelDefinition] = &[
    QWEN_06B_BASE,
    QWEN_17B_BASE,
    QWEN_06B_CUSTOM_VOICE,
    QWEN_17B_CUSTOM_VOICE,
    QWEN_17B_VOICE_DESIGN,
];

pub const QWEN_BASE_DEFAULT_MODEL_ID: &str = QWEN_06B_BASE.id;
pub const QWEN_CUSTOM_VOICE_DEFAULT_MODEL_ID: &str = QWEN_06B_CUSTOM_VOICE.id;
pub const QWEN_VOICE_DESIGN_DEFAULT_MODEL_ID: &str = QWEN_17B_VOICE_DESIGN.id;

pub fn default_qwen_model(mode: QwenTtsMode) -> &'static TtsModelDefinition {
    match mode {
        QwenTtsMode::Base => &QWEN_06B_BASE,
        QwenTtsMode::CustomVoice => &QWEN_06B_CUSTOM_VOICE,
        QwenTtsMode::VoiceDesign => &QWEN_17B_VOICE_DESIGN,
    }
}

pub fn lookup_tts(id: &str) -> Option<&'static TtsModelDefinition> {
    match id {
        "kokoro-v1.0-int8" => Some(&KOKORO),
        "qwen3-tts-0.6b-base-q8_0" => Some(&QWEN_06B_BASE),
        "qwen3-tts-1.7b-base-q8_0" => Some(&QWEN_17B_BASE),
        "qwen3-tts-0.6b-custom-voice-q8_0" => Some(&QWEN_06B_CUSTOM_VOICE),
        "qwen3-tts-1.7b-custom-voice-q8_0" => Some(&QWEN_17B_CUSTOM_VOICE),
        "qwen3-tts-1.7b-voice-design-q8_0" => Some(&QWEN_17B_VOICE_DESIGN),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedQwenModel {
    pub definition: &'static TtsModelDefinition,
    pub metadata: QwenModelMetadata,
    pub talker_path: PathBuf,
    pub codec_path: PathBuf,
}

fn resolve_qwen_definition(
    cache_root: &Path,
    provider: &str,
    model: &'static TtsModelDefinition,
) -> Result<ResolvedQwenModel, SophonError> {
    if provider != model.provider {
        return Err(SophonError::ModelUnavailable(format!(
            "TTS provider `{provider}` does not own model `{}` (expected `{}`)",
            model.id, model.provider
        )));
    }
    let metadata = model.qwen.ok_or_else(|| {
        SophonError::ModelUnavailable(format!("model `{}` is not a Qwen model", model.id))
    })?;
    if !model.files.contains(&metadata.talker) || !model.files.contains(&metadata.codec) {
        return Err(SophonError::ModelUnavailable(format!(
            "Qwen model `{}` has inconsistent artifact roles",
            model.id
        )));
    }
    let talker_path = artifact_path(cache_root, &metadata.talker)?;
    let codec_path = artifact_path(cache_root, &metadata.codec)?;
    validate_artifact(&talker_path, metadata.talker.identity())?;
    validate_artifact(&codec_path, metadata.codec.identity())?;
    Ok(ResolvedQwenModel {
        definition: model,
        metadata,
        talker_path,
        codec_path,
    })
}

pub fn resolve_qwen_model(
    cache_root: &Path,
    provider: &str,
    model_id: &str,
) -> Result<ResolvedQwenModel, SophonError> {
    let model = lookup_tts(model_id)
        .ok_or_else(|| SophonError::ModelUnavailable(format!("unknown TTS model `{model_id}`")))?;
    resolve_qwen_definition(cache_root, provider, model)
}

const PARAKEET_FILES: &[ModelFile] = &[
    ModelFile {
        relative_path: "encoder-model.int8.onnx",
        url: "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/resolve/8f23f0c03c8761650bdb5b40aaf3e40d2c15f1ce/encoder-model.int8.onnx",
        sha256: "6139d2fa7e1b086097b277c7149725edbab89cc7c7ae64b23c741be4055aff09",
        expected_size: 652_183_999,
    },
    ModelFile {
        relative_path: "decoder_joint-model.int8.onnx",
        url: "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/resolve/8f23f0c03c8761650bdb5b40aaf3e40d2c15f1ce/decoder_joint-model.int8.onnx",
        sha256: "eea7483ee3d1a30375daedc8ed83e3960c91b098812127a0d99d1c8977667a70",
        expected_size: 18_202_004,
    },
    ModelFile {
        relative_path: "nemo128.onnx",
        url: "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/resolve/8f23f0c03c8761650bdb5b40aaf3e40d2c15f1ce/nemo128.onnx",
        sha256: "a9fde1486ebfcc08f328d75ad4610c67835fea58c73ba57e3209a6f6cf019e9f",
        expected_size: 139_764,
    },
    ModelFile {
        relative_path: "vocab.txt",
        url: "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/resolve/8f23f0c03c8761650bdb5b40aaf3e40d2c15f1ce/vocab.txt",
        sha256: "d58544679ea4bc6ac563d1f545eb7d474bd6cfa467f0a6e2c1dc1c7d37e3c35d",
        expected_size: 93_939,
    },
];

const CANARY_FILES: &[ModelFile] = &[
    ModelFile {
        relative_path: "encoder-model.int8.onnx",
        url: "https://huggingface.co/istupakov/canary-180m-flash-onnx/resolve/92c2231a4e2b2524277fea759be967d2e6edfc49/encoder-model.int8.onnx",
        sha256: "996d1c89e6cbc891a7c88bf410884c178ffa474f7b13084522ac74a5e144cc81",
        expected_size: 133_710_896,
    },
    ModelFile {
        relative_path: "decoder-model.int8.onnx",
        url: "https://huggingface.co/istupakov/canary-180m-flash-onnx/resolve/92c2231a4e2b2524277fea759be967d2e6edfc49/decoder-model.int8.onnx",
        sha256: "9dd9c447872088c912e916d73751f9621a54085d5bc46788454fe904db51a914",
        expected_size: 79_520_211,
    },
    ModelFile {
        relative_path: "nemo128.onnx",
        url: "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/resolve/8f23f0c03c8761650bdb5b40aaf3e40d2c15f1ce/nemo128.onnx",
        sha256: "a9fde1486ebfcc08f328d75ad4610c67835fea58c73ba57e3209a6f6cf019e9f",
        expected_size: 139_764,
    },
    ModelFile {
        relative_path: "vocab.txt",
        url: "https://huggingface.co/istupakov/canary-180m-flash-onnx/resolve/92c2231a4e2b2524277fea759be967d2e6edfc49/vocab.txt",
        sha256: "2dae6fc7815f9640645e0c765522b278ee0cef49b482d91f6913e334628d3e77",
        expected_size: 53_555,
    },
];

pub const PARAKEET: ModelDefinition = ModelDefinition {
    id: "parakeet-tdt-0.6b-v3-int8",
    engine: Engine::Parakeet,
    revision: "8f23f0c03c8761650bdb5b40aaf3e40d2c15f1ce",
    quantization: Quantization::Int8,
    files: PARAKEET_FILES,
    capabilities: ModelCapabilities {
        languages: &["en"],
        translation_to_english: false,
    },
};
pub const CANARY: ModelDefinition = ModelDefinition {
    id: "canary-180m-flash-en-es-de-fr-int8",
    engine: Engine::Canary,
    revision: "92c2231a4e2b2524277fea759be967d2e6edfc49",
    quantization: Quantization::Int8,
    files: CANARY_FILES,
    capabilities: ModelCapabilities {
        languages: &["en", "es", "de", "fr"],
        translation_to_english: true,
    },
};

pub fn lookup(id: &str) -> Option<&'static ModelDefinition> {
    match id {
        "parakeet-tdt-0.6b-v3-int8" => Some(&PARAKEET),
        "canary-180m-flash-en-es-de-fr-int8" => Some(&CANARY),
        _ => None,
    }
}

pub fn validate_layout<M: ArtifactManifest + ?Sized>(
    root: &Path,
    model: &M,
) -> Result<(), SophonError> {
    for file in model.files() {
        let path = root.join(file.relative_path);
        if !path.is_file() {
            return Err(SophonError::ModelUnavailable(format!(
                "model layout is missing {}",
                path.display()
            )));
        }
    }
    Ok(())
}

pub fn cache_path<M: ArtifactManifest + ?Sized>(cache_root: &Path, model: &M) -> PathBuf {
    cache_root.join(model.id()).join(model.revision())
}

/// Returns the content-addressed cache path for one manifest file.
pub fn artifact_path(cache_root: &Path, file: &ModelFile) -> Result<PathBuf, SophonError> {
    let filename = Path::new(file.relative_path).file_name().ok_or_else(|| {
        SophonError::ModelUnavailable(format!(
            "artifact path `{}` has no filename",
            file.relative_path
        ))
    })?;
    Ok(cache_root
        .join("artifacts")
        .join(file.identity().sha256)
        .join(filename))
}

/// Independently verifies an artifact's exact byte size and SHA-256 digest.
pub fn validate_artifact(path: &Path, identity: ArtifactIdentity) -> Result<(), SophonError> {
    let metadata =
        fs::metadata(path).map_err(|error| SophonError::ModelUnavailable(error.to_string()))?;
    if !metadata.is_file() {
        return Err(SophonError::ModelUnavailable(format!(
            "artifact is not a regular file: {}",
            path.display()
        )));
    }
    if metadata.len() != identity.expected_size {
        return Err(SophonError::ModelUnavailable(format!(
            "size mismatch for {}: expected {} bytes, found {}",
            path.display(),
            identity.expected_size,
            metadata.len()
        )));
    }

    let mut input =
        File::open(path).map_err(|error| SophonError::ModelUnavailable(error.to_string()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = input
            .read(&mut buffer)
            .map_err(|error| SophonError::ModelUnavailable(error.to_string()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    if format!("{:x}", hasher.finalize()) != identity.sha256 {
        return Err(SophonError::ModelUnavailable(format!(
            "SHA-256 mismatch for {}",
            path.display()
        )));
    }
    Ok(())
}

pub fn validate_manifest<M: ArtifactManifest + ?Sized>(
    root: &Path,
    model: &M,
) -> Result<(), SophonError> {
    validate_layout(root, model)?;
    for file in model.files() {
        validate_artifact(&root.join(file.relative_path), file.identity())?;
    }
    Ok(())
}

fn validated_artifacts<M: ArtifactManifest + ?Sized>(
    cache_root: &Path,
    model: &M,
) -> Option<Vec<PathBuf>> {
    model
        .files()
        .iter()
        .map(|file| {
            let path = artifact_path(cache_root, file).ok()?;
            validate_artifact(&path, file.identity()).ok()?;
            Some(path)
        })
        .collect()
}

fn publish_model_view<M: ArtifactManifest + ?Sized>(
    cache_root: &Path,
    model: &M,
) -> Result<PathBuf, SophonError> {
    let destination = cache_path(cache_root, model);
    if validate_manifest(&destination, model).is_ok() {
        return Ok(destination);
    }
    let artifacts = validated_artifacts(cache_root, model).ok_or_else(|| {
        SophonError::ModelUnavailable(format!("not all artifacts for `{}` are valid", model.id()))
    })?;
    let temporary = tempfile::Builder::new()
        .prefix(".sophon-view-")
        .tempdir_in(cache_root)
        .map_err(|error| SophonError::ModelUnavailable(error.to_string()))?;
    for (file, artifact) in model.files().iter().zip(artifacts) {
        let link = temporary.path().join(file.relative_path);
        if let Some(parent) = link.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| SophonError::ModelUnavailable(error.to_string()))?;
        }
        fs::hard_link(artifact, link)
            .map_err(|error| SophonError::ModelUnavailable(error.to_string()))?;
    }
    validate_manifest(temporary.path(), model)?;
    if destination.exists() {
        fs::remove_dir_all(&destination)
            .map_err(|error| SophonError::ModelUnavailable(error.to_string()))?;
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| SophonError::ModelUnavailable(error.to_string()))?;
    }
    fs::rename(temporary.path(), &destination)
        .map_err(|error| SophonError::ModelUnavailable(error.to_string()))?;
    Ok(destination)
}

/// Returns a cache entry only after its complete manifest and expected layout
/// have been verified. Invalid or partial entries are never usable offline.
pub fn validated_cache<M: ArtifactManifest + ?Sized>(
    cache_root: &Path,
    model: &'static M,
) -> Option<PathBuf> {
    let legacy_or_view = cache_path(cache_root, model);
    if validate_manifest(&legacy_or_view, model).is_ok() {
        return Some(legacy_or_view);
    }
    publish_model_view(cache_root, model).ok()
}

async fn lock_file(path: PathBuf, operation: &'static str) -> Result<File, SophonError> {
    tokio::task::spawn_blocking(move || -> Result<File, SophonError> {
        let lock =
            File::create(path).map_err(|error| SophonError::ModelUnavailable(error.to_string()))?;
        lock.lock_exclusive()
            .map_err(|error| SophonError::ModelUnavailable(error.to_string()))?;
        Ok(lock)
    })
    .await
    .map_err(|error| {
        SophonError::ModelUnavailable(format!("{operation} lock task failed: {error}"))
    })?
}

async fn acquire_artifact<F>(
    cache_root: &Path,
    client: &reqwest::Client,
    file: &ModelFile,
    completed_bytes: u64,
    total_bytes: u64,
    progress: &mut F,
) -> Result<PathBuf, SophonError>
where
    F: FnMut(f32),
{
    let destination = artifact_path(cache_root, file)?;
    if validate_artifact(&destination, file.identity()).is_ok() {
        return Ok(destination);
    }
    let artifacts_root = cache_root.join("artifacts");
    fs::create_dir_all(&artifacts_root)
        .map_err(|error| SophonError::ModelUnavailable(error.to_string()))?;
    let _lock = lock_file(
        artifacts_root.join(format!(".{}.lock", file.sha256)),
        "artifact-cache",
    )
    .await?;
    if validate_artifact(&destination, file.identity()).is_ok() {
        return Ok(destination);
    }
    let parent = destination.parent().ok_or_else(|| {
        SophonError::ModelUnavailable(format!("artifact has no parent: {}", destination.display()))
    })?;
    fs::create_dir_all(parent).map_err(|error| SophonError::ModelUnavailable(error.to_string()))?;
    let mut temporary = tempfile::Builder::new()
        .prefix(".sophon-download-")
        .tempfile_in(parent)
        .map_err(|error| SophonError::ModelUnavailable(error.to_string()))?;
    let response = client
        .get(file.url)
        .send()
        .await
        .map_err(|error| SophonError::ModelUnavailable(format!("download {}: {error}", file.url)))?
        .error_for_status()
        .map_err(|error| {
            SophonError::ModelUnavailable(format!("download {}: {error}", file.url))
        })?;
    let mut stream = response.bytes_stream();
    let mut hasher = Sha256::new();
    let mut downloaded = 0_u64;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| {
            SophonError::ModelUnavailable(format!("download {}: {error}", file.url))
        })?;
        downloaded = downloaded.checked_add(chunk.len() as u64).ok_or_else(|| {
            SophonError::ModelUnavailable(format!("download size overflow for {}", file.url))
        })?;
        hasher.update(&chunk);
        temporary
            .write_all(&chunk)
            .map_err(|error| SophonError::ModelUnavailable(error.to_string()))?;
        let credited = completed_bytes.saturating_add(downloaded.min(file.expected_size));
        progress((credited as f64 / total_bytes as f64) as f32);
    }
    temporary
        .flush()
        .and_then(|()| temporary.as_file().sync_all())
        .map_err(|error| SophonError::ModelUnavailable(error.to_string()))?;
    if downloaded != file.expected_size {
        return Err(SophonError::ModelUnavailable(format!(
            "size mismatch for {}: expected {} bytes, downloaded {}",
            file.relative_path, file.expected_size, downloaded
        )));
    }
    if format!("{:x}", hasher.finalize()) != file.sha256 {
        return Err(SophonError::ModelUnavailable(format!(
            "SHA-256 mismatch for {}",
            file.relative_path
        )));
    }
    if destination.exists() {
        fs::remove_file(&destination)
            .map_err(|error| SophonError::ModelUnavailable(error.to_string()))?;
    }
    temporary
        .persist(&destination)
        .map_err(|error| SophonError::ModelUnavailable(error.error.to_string()))?;
    validate_artifact(&destination, file.identity())?;
    Ok(destination)
}

/// Acquires every manifest file as an independently locked, verified artifact
/// and publishes a hard-linked compatibility view for directory-based loaders.
pub async fn acquire<M, F>(
    cache_root: &Path,
    model: &'static M,
    mut progress: F,
) -> Result<PathBuf, SophonError>
where
    M: ArtifactManifest + ?Sized,
    F: FnMut(f32),
{
    if let Some(path) = validated_cache(cache_root, model) {
        progress(1.0);
        return Ok(path);
    }
    fs::create_dir_all(cache_root)
        .map_err(|error| SophonError::ModelUnavailable(error.to_string()))?;
    let total_bytes = model.files().iter().try_fold(0_u64, |total, file| {
        total.checked_add(file.expected_size).ok_or_else(|| {
            SophonError::ModelUnavailable(format!("manifest size overflow for `{}`", model.id()))
        })
    })?;
    if total_bytes == 0 {
        return Err(SophonError::ModelUnavailable(format!(
            "manifest `{}` contains no artifact bytes",
            model.id()
        )));
    }
    let client = reqwest::Client::new();
    let mut completed_bytes = 0_u64;
    for file in model.files() {
        acquire_artifact(
            cache_root,
            &client,
            file,
            completed_bytes,
            total_bytes,
            &mut progress,
        )
        .await?;
        completed_bytes += file.expected_size;
        progress((completed_bytes as f64 / total_bytes as f64) as f32);
    }
    let views_root = cache_root.join(".views");
    fs::create_dir_all(&views_root)
        .map_err(|error| SophonError::ModelUnavailable(error.to_string()))?;
    let _view_lock = lock_file(
        views_root.join(format!(".{}.lock", model.id())),
        "model-view",
    )
    .await?;
    publish_model_view(cache_root, model)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelLocation {
    LocalOverride(PathBuf),
    Registry(&'static ModelDefinition),
}

/// Resolves a local override before consulting the curated registry. An invalid
/// override is terminal: callers must not silently download a different model.
pub fn resolve_location(
    model_id: &str,
    override_path: Option<&Path>,
) -> Result<ModelLocation, SophonError> {
    if let Some(path) = override_path {
        let model = lookup(model_id)
            .ok_or_else(|| SophonError::ModelUnavailable(format!("unknown model `{model_id}`")))?;
        validate_layout(path, model)?;
        return Ok(ModelLocation::LocalOverride(path.to_path_buf()));
    }
    lookup(model_id)
        .map(ModelLocation::Registry)
        .ok_or_else(|| SophonError::ModelUnavailable(format!("unknown model `{model_id}`")))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TtsModelLocation {
    LocalOverride(PathBuf),
    Registry(&'static TtsModelDefinition),
}

/// Resolves TTS overrides independently. A present invalid override is terminal
/// and can never trigger an automatic registry download.
pub fn resolve_tts_location(
    provider: &str,
    model_id: &str,
    override_path: Option<&Path>,
) -> Result<TtsModelLocation, SophonError> {
    let model = lookup_tts(model_id)
        .ok_or_else(|| SophonError::ModelUnavailable(format!("unknown TTS model `{model_id}`")))?;
    if model.provider != provider {
        return Err(SophonError::ModelUnavailable(format!(
            "TTS provider `{provider}` does not own model `{model_id}` (expected `{}`)",
            model.provider
        )));
    }
    if let Some(path) = override_path {
        validate_manifest(path, model)?;
        return Ok(TtsModelLocation::LocalOverride(path.to_path_buf()));
    }
    Ok(TtsModelLocation::Registry(model))
}

/// Resolves a curated local Qwen override without consulting or falling back to
/// the registry cache. Both role files must exactly match the selected model.
pub fn resolve_qwen_override(
    root: &Path,
    provider: &str,
    model_id: &str,
) -> Result<ResolvedQwenModel, SophonError> {
    let model = lookup_tts(model_id)
        .ok_or_else(|| SophonError::ModelUnavailable(format!("unknown TTS model `{model_id}`")))?;
    if model.provider != provider {
        return Err(SophonError::ModelUnavailable(format!(
            "TTS provider `{provider}` does not own model `{model_id}` (expected `{}`)",
            model.provider
        )));
    }
    let metadata = model.qwen.ok_or_else(|| {
        SophonError::ModelUnavailable(format!("model `{model_id}` is not a Qwen model"))
    })?;
    let talker_path = root.join(metadata.talker.relative_path);
    let codec_path = root.join(metadata.codec.relative_path);
    validate_artifact(&talker_path, metadata.talker.identity())?;
    validate_artifact(&codec_path, metadata.codec.identity())?;
    Ok(ResolvedQwenModel {
        definition: model,
        metadata,
        talker_path,
        codec_path,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tts_registry_pins_exact_kokoro_release_layout_and_digests() {
        assert_eq!(KOKORO.provider, "tts-rs");
        assert_eq!(KOKORO.revision, "model-files-v1.0");
        assert_eq!(
            KOKORO
                .files
                .iter()
                .map(|file| file.relative_path)
                .collect::<Vec<_>>(),
            ["kokoro-v1.0.int8.onnx", "voices-v1.0.bin"]
        );
        assert_eq!(
            KOKORO
                .files
                .iter()
                .map(|file| file.expected_size)
                .collect::<Vec<_>>(),
            [92_361_271, 28_214_398]
        );
        assert!(KOKORO.files.iter().all(|file| {
            file.url.starts_with(
                "https://github.com/thewh1teagle/kokoro-onnx/releases/download/model-files-v1.0/",
            ) && file.sha256.len() == 64
                && file
                    .sha256
                    .chars()
                    .all(|character| character.is_ascii_hexdigit())
                && file.identity()
                    == (ArtifactIdentity {
                        sha256: file.sha256,
                        expected_size: file.expected_size,
                    })
        }));
    }

    #[test]
    fn qwen_catalog_pins_exact_q8_0_artifact_metadata() {
        assert_eq!(QWEN_REVISION, "e0f336a048a3de02b29b8ad92969217d9ecffe3e");
        assert_eq!(
            QWEN_Q8_0_ARTIFACTS
                .iter()
                .map(|file| (file.relative_path, file.expected_size, file.sha256))
                .collect::<Vec<_>>(),
            vec![
                (
                    "qwen-talker-0.6b-base-Q8_0.gguf",
                    992_615_488,
                    "d54dbaf10591421fa764ed630d764efa717ae40cd959bd48c66d4eb1af226426",
                ),
                (
                    "qwen-talker-0.6b-customvoice-Q8_0.gguf",
                    968_588_544,
                    "4eb38675c736ed6ac72012846ac8d6ef80e5af8bc05726870f0b3a6569588519",
                ),
                (
                    "qwen-talker-1.7b-base-Q8_0.gguf",
                    2_079_448_256,
                    "4b9a33a236908dd9435a42f7a396e38038329d053b704342a6413c08544c4fda",
                ),
                (
                    "qwen-talker-1.7b-customvoice-Q8_0.gguf",
                    2_042_834_304,
                    "cab2cff67a0a557310febe558dc83076b28ed790e491867eb2751759f4cd89fa",
                ),
                (
                    "qwen-talker-1.7b-voicedesign-Q8_0.gguf",
                    2_042_833_824,
                    "575610ab1ddcca4dca6bd9a64bcd859d93bbad8764f9cab24e1dbc0c51f62276",
                ),
                (
                    "qwen-tokenizer-12hz-Q8_0.gguf",
                    291_150_624,
                    "1883beeed99348fc35e23dd225e9082f93f6f8c109330a33d935baa8acdbfd94",
                ),
            ]
        );
        assert!(QWEN_Q8_0_ARTIFACTS.iter().all(|file| {
            file.url.contains(QWEN_REVISION)
                && file.url.ends_with(file.relative_path)
                && file.sha256.len() == 64
        }));
    }

    #[test]
    fn qwen_models_have_stable_typed_modes_and_mode_defaults() {
        assert_eq!(QWEN_MODELS.len(), 5);
        assert_eq!(
            QWEN_MODELS.iter().map(|model| model.id).collect::<Vec<_>>(),
            [
                "qwen3-tts-0.6b-base-q8_0",
                "qwen3-tts-1.7b-base-q8_0",
                "qwen3-tts-0.6b-custom-voice-q8_0",
                "qwen3-tts-1.7b-custom-voice-q8_0",
                "qwen3-tts-1.7b-voice-design-q8_0",
            ]
        );
        assert_eq!(
            default_qwen_model(QwenTtsMode::Base).id,
            QWEN_BASE_DEFAULT_MODEL_ID
        );
        assert_eq!(
            default_qwen_model(QwenTtsMode::CustomVoice).id,
            QWEN_CUSTOM_VOICE_DEFAULT_MODEL_ID
        );
        assert_eq!(
            default_qwen_model(QwenTtsMode::VoiceDesign).id,
            QWEN_VOICE_DESIGN_DEFAULT_MODEL_ID
        );
        assert_eq!(QWEN_06B_BASE.qwen.unwrap().size, QwenModelSize::Billion06);
        assert_eq!(QWEN_17B_BASE.qwen.unwrap().size, QwenModelSize::Billion17);
        assert_eq!(
            QWEN_06B_CUSTOM_VOICE.qwen.unwrap().mode,
            QwenTtsMode::CustomVoice
        );
        assert_eq!(
            QWEN_17B_VOICE_DESIGN.qwen.unwrap().mode,
            QwenTtsMode::VoiceDesign
        );
        assert!(
            QWEN_MODELS
                .iter()
                .all(|model| model.provider == "qwentts-cpp"
                    && model.qwen.unwrap().quantization == "Q8_0"
                    && lookup_tts(model.id) == Some(model))
        );
        assert_eq!(
            QWEN_MODELS
                .iter()
                .map(|model| model.qwen.unwrap().codec.identity())
                .collect::<std::collections::HashSet<_>>()
                .len(),
            1
        );
        assert_eq!(
            QWEN_MODELS
                .iter()
                .map(|model| model.qwen.unwrap().talker.identity())
                .collect::<std::collections::HashSet<_>>()
                .len(),
            5
        );
        assert!(lookup_tts("qwen3-tts-unregistered-q8_0").is_none());
        let cache = tempfile::tempdir().unwrap();
        assert!(matches!(
            resolve_qwen_model(cache.path(), "tts-rs", QWEN_06B_BASE.id),
            Err(SophonError::ModelUnavailable(message)) if message.contains("does not own")
        ));
        assert!(matches!(
            resolve_qwen_model(cache.path(), "qwentts-cpp", "qwen3-tts-unregistered-q8_0"),
            Err(SophonError::ModelUnavailable(message)) if message.contains("unknown")
        ));
    }

    #[test]
    fn qwen_resolution_returns_verified_role_paths_and_enforces_provider_agreement() {
        let model = fixture_qwen_model(b"talker", b"codec");
        let cache = tempfile::tempdir().unwrap();
        for file in model.files {
            let path = artifact_path(cache.path(), file).unwrap();
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(
                path,
                match file.relative_path {
                    "talker.gguf" => b"talker".as_slice(),
                    "codec.gguf" => b"codec".as_slice(),
                    _ => unreachable!(),
                },
            )
            .unwrap();
        }
        let resolved = resolve_qwen_definition(cache.path(), "qwentts-cpp", model).unwrap();
        assert_eq!(
            resolved.talker_path,
            artifact_path(cache.path(), &model.qwen.unwrap().talker).unwrap()
        );
        assert_eq!(
            resolved.codec_path,
            artifact_path(cache.path(), &model.qwen.unwrap().codec).unwrap()
        );
        assert!(matches!(
            resolve_qwen_definition(cache.path(), "tts-rs", model),
            Err(SophonError::ModelUnavailable(message)) if message.contains("does not own")
        ));
        std::fs::remove_file(&resolved.codec_path).unwrap();
        assert!(resolve_qwen_definition(cache.path(), "qwentts-cpp", model).is_err());
    }

    #[test]
    fn qwen_local_overrides_are_curated_exact_and_never_fall_back() {
        let directory = tempfile::tempdir().unwrap();
        let metadata = QWEN_06B_BASE.qwen.unwrap();
        std::fs::write(
            directory.path().join(metadata.talker.relative_path),
            b"self-converted talker",
        )
        .unwrap();
        std::fs::write(
            directory.path().join(metadata.codec.relative_path),
            b"self-converted codec",
        )
        .unwrap();
        assert!(matches!(
            resolve_qwen_override(directory.path(), "qwentts-cpp", QWEN_06B_BASE.id),
            Err(SophonError::ModelUnavailable(message)) if message.contains("mismatch")
        ));
        assert!(matches!(
            resolve_tts_location("tts-rs", QWEN_06B_BASE.id, Some(directory.path())),
            Err(SophonError::ModelUnavailable(message)) if message.contains("does not own")
        ));
        assert!(!directory.path().join("artifacts").exists());
    }

    #[test]
    fn registry_has_pinned_https_files_for_both_engines() {
        for model in [PARAKEET, CANARY] {
            assert!(
                model
                    .files
                    .iter()
                    .all(|file| file.url.starts_with("https://"))
            );
            assert!(model.files.iter().all(|file| {
                file.sha256.len() == 64
                    && file.expected_size > 0
                    && file.identity().sha256 == file.sha256
            }));
        }
    }

    #[test]
    fn invalid_override_does_not_fall_back_to_registry() {
        let path = tempfile::tempdir().unwrap();
        assert!(matches!(
            resolve_location(PARAKEET.id, Some(path.path())),
            Err(SophonError::ModelUnavailable(_))
        ));
    }

    #[test]
    fn invalid_tts_override_is_terminal_and_never_falls_back_to_download() {
        let path = tempfile::tempdir().unwrap();
        assert!(matches!(
            resolve_tts_location(KOKORO.provider, KOKORO.id, Some(path.path())),
            Err(SophonError::ModelUnavailable(_))
        ));

        for file in KOKORO.files {
            std::fs::write(path.path().join(file.relative_path), b"wrong digest").unwrap();
        }
        assert!(matches!(
            resolve_tts_location(KOKORO.provider, KOKORO.id, Some(path.path())),
            Err(SophonError::ModelUnavailable(message)) if message.contains("mismatch")
        ));
    }

    #[test]
    fn artifact_paths_are_content_addressed_by_digest_and_filename() {
        let path = artifact_path(Path::new("/cache"), &KOKORO.files[0]).unwrap();
        assert_eq!(
            path,
            Path::new("/cache")
                .join("artifacts")
                .join(KOKORO.files[0].sha256)
                .join("kokoro-v1.0.int8.onnx")
        );
    }

    #[test]
    fn artifact_validation_rejects_size_and_digest_independently() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("artifact.bin");
        std::fs::write(&path, b"correct bytes").unwrap();
        let digest = Box::leak(format!("{:x}", Sha256::digest(b"correct bytes")).into_boxed_str());
        let identity = ArtifactIdentity {
            sha256: digest,
            expected_size: 13,
        };
        assert!(validate_artifact(&path, identity).is_ok());

        assert!(matches!(
            validate_artifact(
                &path,
                ArtifactIdentity {
                    expected_size: 12,
                    ..identity
                }
            ),
            Err(SophonError::ModelUnavailable(message)) if message.contains("size mismatch")
        ));
        std::fs::write(&path, b"wrong content").unwrap();
        assert!(matches!(
            validate_artifact(&path, identity),
            Err(SophonError::ModelUnavailable(message)) if message.contains("SHA-256 mismatch")
        ));
    }

    #[test]
    fn partial_cache_is_not_usable() {
        let cache = tempfile::tempdir().unwrap();
        let root = cache_path(cache.path(), &PARAKEET);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join(PARAKEET.files[0].relative_path), "partial").unwrap();
        assert_eq!(validated_cache(cache.path(), &PARAKEET), None);
    }

    fn fixture_tts_model(url: String, bytes: &[u8]) -> &'static TtsModelDefinition {
        fixture_tts_model_with_files(
            "fixture-tts-model",
            vec![("model.onnx", url, bytes.to_vec())],
        )
    }

    fn fixture_tts_model_with_files(
        id: &'static str,
        files: Vec<(&'static str, String, Vec<u8>)>,
    ) -> &'static TtsModelDefinition {
        let files = files
            .into_iter()
            .map(|(relative_path, url, bytes)| ModelFile {
                relative_path,
                url: Box::leak(url.into_boxed_str()),
                sha256: Box::leak(format!("{:x}", Sha256::digest(&bytes)).into_boxed_str()),
                expected_size: bytes.len() as u64,
            })
            .collect::<Vec<_>>();
        Box::leak(Box::new(TtsModelDefinition {
            id,
            provider: "fixture",
            revision: "fixture",
            files: Box::leak(files.into_boxed_slice()),
            qwen: None,
        }))
    }

    fn fixture_qwen_model(talker_bytes: &[u8], codec_bytes: &[u8]) -> &'static TtsModelDefinition {
        let model = fixture_tts_model_with_files(
            "fixture-qwen-model",
            vec![
                (
                    "talker.gguf",
                    "http://fixture/talker".into(),
                    talker_bytes.to_vec(),
                ),
                (
                    "codec.gguf",
                    "http://fixture/codec".into(),
                    codec_bytes.to_vec(),
                ),
            ],
        );
        let files = model.files;
        Box::leak(Box::new(TtsModelDefinition {
            id: model.id,
            provider: "qwentts-cpp",
            revision: model.revision,
            files,
            qwen: Some(QwenModelMetadata {
                mode: QwenTtsMode::Base,
                size: QwenModelSize::Billion06,
                quantization: "Q8_0",
                talker: files[0],
                codec: files[1],
            }),
        }))
    }

    fn fixture_server(body: Vec<u8>) -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
        use std::{
            io::{Read as _, Write as _},
            net::TcpListener,
            sync::{
                Arc,
                atomic::{AtomicUsize, Ordering},
            },
            thread,
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let requests = Arc::new(AtomicUsize::new(0));
        let count = requests.clone();
        thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut request = [0; 1024];
            let _ = socket.read(&mut request);
            count.fetch_add(1, Ordering::SeqCst);
            write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .unwrap();
            socket.write_all(&body).unwrap();
        });
        (format!("http://{address}/model.onnx"), requests)
    }

    fn chunked_fixture_server(first: Vec<u8>, second: Vec<u8>) -> String {
        use std::{
            io::{Read as _, Write as _},
            net::TcpListener,
            thread,
            time::Duration,
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut request = [0; 1024];
            let _ = socket.read(&mut request);
            write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                first.len() + second.len()
            )
            .unwrap();
            socket.write_all(&first).unwrap();
            socket.flush().unwrap();
            thread::sleep(Duration::from_millis(30));
            socket.write_all(&second).unwrap();
        });
        format!("http://{address}/model.onnx")
    }

    fn interrupted_fixture_server() -> String {
        use std::{
            io::{Read as _, Write as _},
            net::TcpListener,
            thread,
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut request = [0; 1024];
            let _ = socket.read(&mut request);
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 32\r\nConnection: close\r\n\r\npartial",
                )
                .unwrap();
        });
        format!("http://{address}/model.onnx")
    }

    #[tokio::test]
    async fn pre_change_valid_tts_cache_remains_usable_without_downloading() {
        let bytes = b"pre-change Kokoro fixture";
        let model = fixture_tts_model_with_files(
            "fixture-pre-change-kokoro",
            vec![(
                "kokoro-v1.0.int8.onnx",
                "http://127.0.0.1:9/must-not-download".into(),
                bytes.to_vec(),
            )],
        );
        let cache = tempfile::tempdir().unwrap();
        let legacy = cache_path(cache.path(), model);
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::write(legacy.join("kokoro-v1.0.int8.onnx"), bytes).unwrap();

        assert_eq!(validated_cache(cache.path(), model), Some(legacy.clone()));
        let mut progress = Vec::new();
        assert_eq!(
            acquire(cache.path(), model, |value| progress.push(value))
                .await
                .unwrap(),
            legacy
        );
        assert_eq!(progress, [1.0]);
    }

    #[tokio::test]
    async fn download_is_verified_published_and_reused_offline() {
        let bytes = b"verified fixture";
        let (url, requests) = fixture_server(bytes.to_vec());
        let model = fixture_tts_model(url, bytes);
        let cache = tempfile::tempdir().unwrap();
        let path = acquire(cache.path(), model, |_| {}).await.unwrap();
        assert_eq!(std::fs::read(path.join("model.onnx")).unwrap(), bytes);
        assert_eq!(acquire(cache.path(), model, |_| {}).await.unwrap(), path);
        assert_eq!(requests.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn acquisition_reports_monotonic_byte_progress_and_credits_cached_artifacts() {
        let first = vec![b'a'; 32 * 1024];
        let second = vec![b'b'; 32 * 1024];
        let bytes = [first.as_slice(), second.as_slice()].concat();
        let model = fixture_tts_model(chunked_fixture_server(first, second), &bytes);
        let cache = tempfile::tempdir().unwrap();
        let mut progress = Vec::new();
        acquire(cache.path(), model, |value| progress.push(value))
            .await
            .unwrap();
        assert!(progress.windows(2).all(|pair| pair[0] <= pair[1]));
        assert!(progress.iter().any(|value| *value > 0.0 && *value < 1.0));
        assert_eq!(progress.last(), Some(&1.0));

        let mut cached_progress = Vec::new();
        acquire(cache.path(), model, |value| cached_progress.push(value))
            .await
            .unwrap();
        assert_eq!(cached_progress, [1.0]);
    }

    #[tokio::test]
    async fn models_reuse_one_shared_content_addressed_artifact() {
        let bytes = b"shared artifact";
        let (url, requests) = fixture_server(bytes.to_vec());
        let first = fixture_tts_model_with_files(
            "fixture-shared-first",
            vec![("model.onnx", url, bytes.to_vec())],
        );
        let second = fixture_tts_model_with_files(
            "fixture-shared-second",
            vec![(
                "model.onnx",
                "http://127.0.0.1:9/must-not-download".into(),
                bytes.to_vec(),
            )],
        );
        let cache = tempfile::tempdir().unwrap();
        let first_view = acquire(cache.path(), first, |_| {}).await.unwrap();
        let second_view = acquire(cache.path(), second, |_| {}).await.unwrap();
        assert_eq!(requests.load(std::sync::atomic::Ordering::SeqCst), 1);
        let artifact = artifact_path(cache.path(), &first.files[0]).unwrap();
        assert_eq!(
            artifact,
            artifact_path(cache.path(), &second.files[0]).unwrap()
        );
        assert_eq!(std::fs::read(&artifact).unwrap(), bytes);
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let inode = std::fs::metadata(artifact).unwrap().ino();
            assert_eq!(
                std::fs::metadata(first_view.join("model.onnx"))
                    .unwrap()
                    .ino(),
                inode
            );
            assert_eq!(
                std::fs::metadata(second_view.join("model.onnx"))
                    .unwrap()
                    .ino(),
                inode
            );
        }
    }

    #[tokio::test]
    async fn corrupt_cached_artifact_is_rejected_and_atomically_replaced() {
        let expected = b"expected content";
        let (url, requests) = fixture_server(expected.to_vec());
        let model = fixture_tts_model_with_files(
            "fixture-corrupt-cache",
            vec![("model.onnx", url, expected.to_vec())],
        );
        let cache = tempfile::tempdir().unwrap();
        let artifact = artifact_path(cache.path(), &model.files[0]).unwrap();
        std::fs::create_dir_all(artifact.parent().unwrap()).unwrap();
        std::fs::write(&artifact, b"corrupted bytes!").unwrap();
        assert!(validate_artifact(&artifact, model.files[0].identity()).is_err());

        acquire(cache.path(), model, |_| {}).await.unwrap();
        assert_eq!(requests.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(std::fs::read(&artifact).unwrap(), expected);
        assert!(validate_artifact(&artifact, model.files[0].identity()).is_ok());
    }

    #[tokio::test]
    async fn failed_later_download_retains_completed_artifacts_and_cleans_temporary_files() {
        let completed = b"completed artifact";
        let missing = b"eventual complete artifact";
        let (completed_url, _) = fixture_server(completed.to_vec());
        let model = fixture_tts_model_with_files(
            "fixture-partial-model",
            vec![
                ("completed.onnx", completed_url, completed.to_vec()),
                (
                    "missing.onnx",
                    interrupted_fixture_server(),
                    missing.to_vec(),
                ),
            ],
        );
        let cache = tempfile::tempdir().unwrap();
        assert!(acquire(cache.path(), model, |_| {}).await.is_err());

        let completed_path = artifact_path(cache.path(), &model.files[0]).unwrap();
        assert!(validate_artifact(&completed_path, model.files[0].identity()).is_ok());
        let missing_path = artifact_path(cache.path(), &model.files[1]).unwrap();
        assert!(!missing_path.exists());
        let missing_parent = missing_path.parent().unwrap();
        assert!(std::fs::read_dir(missing_parent).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".sophon-download-")
        }));
    }

    #[test]
    fn lifecycle_exposes_progress_and_failures() {
        let lifecycle = ModelLifecycle::new();
        lifecycle.downloading(0.5);
        assert_eq!(lifecycle.snapshot().download_progress, 0.5);
        lifecycle.loading(&PARAKEET);
        lifecycle.ready();
        assert!(matches!(
            lifecycle.snapshot().state,
            crate::domain::ModelState::Ready
        ));
        lifecycle.failed("network unavailable");
        assert_eq!(
            lifecycle.snapshot().last_error.as_deref(),
            Some("network unavailable")
        );
    }

    #[tokio::test]
    async fn digest_mismatch_never_publishes_a_cache_entry() {
        let (url, _) = fixture_server(b"wrong bytes".to_vec());
        let model = fixture_tts_model(url, b"expected bytes");
        let cache = tempfile::tempdir().unwrap();
        assert!(matches!(
            acquire(cache.path(), model, |_| {}).await,
            Err(SophonError::ModelUnavailable(_))
        ));
        assert!(!cache_path(cache.path(), model).exists());
    }

    #[tokio::test]
    async fn interrupted_download_leaves_no_cache_and_a_later_attempt_recovers() {
        let bytes = b"complete fixture";
        let model = fixture_tts_model(interrupted_fixture_server(), bytes);
        let cache = tempfile::tempdir().unwrap();
        assert!(matches!(
            acquire(cache.path(), model, |_| {}).await,
            Err(SophonError::ModelUnavailable(_))
        ));
        assert!(!cache_path(cache.path(), model).exists());

        let (url, _) = fixture_server(bytes.to_vec());
        let recovered = fixture_tts_model(url, bytes);
        assert!(acquire(cache.path(), recovered, |_| {}).await.is_ok());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn concurrent_acquisition_publishes_one_verified_cache_entry() {
        let bytes = b"concurrent fixture";
        let (url, requests) = fixture_server(bytes.to_vec());
        let model = fixture_tts_model(url, bytes);
        let cache = tempfile::tempdir().unwrap();
        let (first, second) = tokio::join!(
            acquire(cache.path(), model, |_| {}),
            acquire(cache.path(), model, |_| {}),
        );
        assert_eq!(first.unwrap(), second.unwrap());
        assert_eq!(requests.load(std::sync::atomic::Ordering::SeqCst), 1);
    }
}
