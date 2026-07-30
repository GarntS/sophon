//! Startup-only configuration loading, XDG path discovery, and validation.

use std::{env, fs, path::PathBuf};

use directories::BaseDirs;
use serde::Deserialize;
use thiserror::Error;

use crate::model_registry::{LoaderKind, ModelCatalog, package_registry_path};

pub const DEFAULT_STT_PROVIDER: &str = "transcribe-rs";
pub const DEFAULT_MODEL_ID: &str = "parakeet-tdt-0.6b-v3-int8";
pub const DEFAULT_MAX_AUDIO_BYTES: u64 = 32 * 1024 * 1024;
pub const DEFAULT_MAX_AUDIO_SECONDS: u64 = 10 * 60;
pub const DEFAULT_QUEUE_CAPACITY: usize = 8;
pub const DEFAULT_TTS_PROVIDER: &str = "tts-rs";
pub const DEFAULT_TTS_MODEL_ID: &str = "kokoro-v1.0-int8";
pub const DEFAULT_QWEN_BASE_MODEL_ID: &str = "qwen3-tts-0.6b-base-q8_0";
pub const DEFAULT_TTS_VOICE: &str = "af_heart";
pub const DEFAULT_QWEN_CUSTOM_VOICE: &str = "vivian";
pub const DEFAULT_QWEN_VOICE_DESCRIPTION: &str =
    "A warm, clear, natural adult voice with moderate pitch and pace.";
pub const DEFAULT_TTS_SPEED: f64 = 1.0;
pub const DEFAULT_TTS_VOLUME: f64 = 1.0;
pub const DEFAULT_MAX_TEXT_BYTES: u64 = 16 * 1024;
pub const DEFAULT_MAX_REFERENCE_AUDIO_BYTES: u64 = 32 * 1024 * 1024;
pub const DEFAULT_MAX_REFERENCE_AUDIO_SECONDS: u64 = 60;
pub const DEFAULT_MAX_GENERATED_AUDIO_SECONDS: u64 = 10 * 60;
pub const DEFAULT_TTS_QUEUE_CAPACITY: usize = 8;
const MAX_AUDIO_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_AUDIO_SECONDS: u64 = 60 * 60;
const MAX_QUEUE_CAPACITY: usize = 128;
const MAX_TEXT_BYTES: u64 = 16 * 1024 * 1024;
const MAX_QWEN_NEW_TOKENS: u32 = 32 * 1024;
const MAX_QWEN_TOP_K: u32 = 1_000;
const MIN_TTS_SPEED: f64 = 0.5;
const MAX_TTS_SPEED: f64 = 2.0;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("could not determine the user's home directory")]
    HomeUnavailable,
    #[error("could not read configuration `{path}`: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("invalid configuration `{path}`: {source}")]
    Parse {
        path: PathBuf,
        source: serde_yaml::Error,
    },
    #[error("invalid configuration: {0}")]
    Invalid(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigPaths {
    pub config_file: PathBuf,
    pub model_cache: PathBuf,
}

impl ConfigPaths {
    pub fn discover() -> Result<Self, ConfigError> {
        let base = BaseDirs::new().ok_or(ConfigError::HomeUnavailable)?;
        let config_home = env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| base.home_dir().join(".config"));
        let cache_home = env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| base.home_dir().join(".cache"));
        Ok(Self::from_homes(config_home, cache_home))
    }

    pub fn from_homes(config_home: PathBuf, cache_home: PathBuf) -> Self {
        Self {
            config_file: config_home.join("sophon/config.yaml"),
            model_cache: cache_home.join("sophon/models"),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Engine {
    #[default]
    Parakeet,
    Canary,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Quantization {
    #[default]
    Int8,
    Fp16,
    Fp32,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Accelerator {
    #[default]
    Auto,
    Cpu,
    Cuda,
    Migraphx,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Error,
    Warn,
    #[default]
    Info,
    Debug,
    Trace,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct QwenSamplingConfig {
    pub seed: Option<u64>,
    pub max_new_tokens: u32,
    pub temperature: f32,
    pub top_k: u32,
    pub top_p: f32,
    pub repetition_penalty: f32,
}

impl Default for QwenSamplingConfig {
    fn default() -> Self {
        Self {
            seed: None,
            max_new_tokens: 2048,
            temperature: 0.9,
            top_k: 50,
            top_p: 1.0,
            repetition_penalty: 1.05,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct TtsFileConfig {
    provider: String,
    model_id: Option<String>,
    default_voice: Option<String>,
    default_clone_reference: Option<PathBuf>,
    default_clone_transcript: Option<String>,
    default_voice_description: Option<String>,
    sampling: Option<QwenSamplingConfig>,
    default_speed: f64,
    pipewire_node: Option<String>,
    volume: f64,
    max_text_bytes: u64,
    max_reference_audio_bytes: u64,
    max_reference_audio_seconds: u64,
    max_generated_audio_seconds: u64,
    queue_capacity: usize,
}

impl Default for TtsFileConfig {
    fn default() -> Self {
        Self {
            provider: DEFAULT_TTS_PROVIDER.into(),
            model_id: None,
            default_voice: None,
            default_clone_reference: None,
            default_clone_transcript: None,
            default_voice_description: None,
            sampling: None,
            default_speed: DEFAULT_TTS_SPEED,
            pipewire_node: None,
            volume: DEFAULT_TTS_VOLUME,
            max_text_bytes: DEFAULT_MAX_TEXT_BYTES,
            max_reference_audio_bytes: DEFAULT_MAX_REFERENCE_AUDIO_BYTES,
            max_reference_audio_seconds: DEFAULT_MAX_REFERENCE_AUDIO_SECONDS,
            max_generated_audio_seconds: DEFAULT_MAX_GENERATED_AUDIO_SECONDS,
            queue_capacity: DEFAULT_TTS_QUEUE_CAPACITY,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TtsOperationalConfig {
    pub cache_dir: PathBuf,
    pub default_speed: f64,
    pub pipewire_node: Option<String>,
    pub volume: f64,
    pub max_text_bytes: u64,
    pub max_reference_audio_bytes: u64,
    pub max_reference_audio_seconds: u64,
    pub max_generated_audio_seconds: u64,
    pub queue_capacity: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TtsProviderConfig {
    Kokoro {
        model_id: String,
        default_voice: String,
    },
    QwenBase {
        model_id: String,
        default_clone_reference: Option<PathBuf>,
        default_clone_transcript: Option<String>,
        sampling: QwenSamplingConfig,
    },
    QwenCustomVoice {
        model_id: String,
        default_voice: String,
        sampling: QwenSamplingConfig,
    },
    QwenVoiceDesign {
        model_id: String,
        default_voice_description: String,
        sampling: QwenSamplingConfig,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct TtsConfig {
    pub operational: TtsOperationalConfig,
    pub provider: TtsProviderConfig,
}

impl TtsConfig {
    pub fn provider_id(&self) -> &'static str {
        match self.provider {
            TtsProviderConfig::Kokoro { .. } => DEFAULT_TTS_PROVIDER,
            TtsProviderConfig::QwenBase { .. }
            | TtsProviderConfig::QwenCustomVoice { .. }
            | TtsProviderConfig::QwenVoiceDesign { .. } => "qwentts-cpp",
        }
    }

    pub fn model_id(&self) -> &str {
        match &self.provider {
            TtsProviderConfig::Kokoro { model_id, .. }
            | TtsProviderConfig::QwenBase { model_id, .. }
            | TtsProviderConfig::QwenCustomVoice { model_id, .. }
            | TtsProviderConfig::QwenVoiceDesign { model_id, .. } => model_id,
        }
    }

    pub fn default_clone(&self) -> Option<(&PathBuf, Option<&str>)> {
        match &self.provider {
            TtsProviderConfig::QwenBase {
                default_clone_reference: Some(path),
                default_clone_transcript,
                ..
            } => Some((path, default_clone_transcript.as_deref())),
            _ => None,
        }
    }

    pub fn default_voice(&self) -> Option<&str> {
        match &self.provider {
            TtsProviderConfig::Kokoro { default_voice, .. }
            | TtsProviderConfig::QwenCustomVoice { default_voice, .. } => Some(default_voice),
            TtsProviderConfig::QwenBase { .. } | TtsProviderConfig::QwenVoiceDesign { .. } => None,
        }
    }

    pub fn sampling(&self) -> Option<&QwenSamplingConfig> {
        match &self.provider {
            TtsProviderConfig::QwenBase { sampling, .. }
            | TtsProviderConfig::QwenCustomVoice { sampling, .. }
            | TtsProviderConfig::QwenVoiceDesign { sampling, .. } => Some(sampling),
            TtsProviderConfig::Kokoro { .. } => None,
        }
    }

    pub fn default_voice_description(&self) -> Option<&str> {
        match &self.provider {
            TtsProviderConfig::QwenVoiceDesign {
                default_voice_description,
                ..
            } => Some(default_voice_description),
            TtsProviderConfig::Kokoro { .. }
            | TtsProviderConfig::QwenBase { .. }
            | TtsProviderConfig::QwenCustomVoice { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct FileConfig {
    provider: String,
    model_id: String,
    accelerator: Accelerator,
    language: String,
    cache_dir: Option<PathBuf>,
    max_audio_bytes: u64,
    max_audio_seconds: u64,
    queue_capacity: usize,
    log_level: LogLevel,
    // Decode this mapping separately so invalid optional TTS configuration does
    // not prevent otherwise valid STT configuration from loading.
    tts: Option<serde_yaml::Value>,
}

impl Default for FileConfig {
    fn default() -> Self {
        Self {
            provider: DEFAULT_STT_PROVIDER.into(),
            model_id: DEFAULT_MODEL_ID.into(),
            accelerator: Accelerator::Auto,
            language: "en".into(),
            cache_dir: None,
            max_audio_bytes: DEFAULT_MAX_AUDIO_BYTES,
            max_audio_seconds: DEFAULT_MAX_AUDIO_SECONDS,
            queue_capacity: DEFAULT_QUEUE_CAPACITY,
            log_level: LogLevel::Info,
            tts: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub provider: String,
    pub model_id: String,
    pub accelerator: Accelerator,
    pub language: String,
    pub cache_dir: PathBuf,
    pub max_audio_bytes: u64,
    pub max_audio_seconds: u64,
    pub queue_capacity: usize,
    pub log_level: LogLevel,
    pub tts: Result<TtsConfig, String>,
}

impl Config {
    /// Loads configuration once at process startup. Callers retain this value;
    /// no file watching or reload path exists.
    pub fn load(paths: &ConfigPaths) -> Result<Self, ConfigError> {
        let catalog = ModelCatalog::load(&package_registry_path())
            .map_err(|error| ConfigError::Invalid(error.to_string()))?;
        Self::load_with_catalog(paths, &catalog)
    }

    pub fn load_with_catalog(
        paths: &ConfigPaths,
        catalog: &ModelCatalog,
    ) -> Result<Self, ConfigError> {
        let file_config = match fs::read_to_string(&paths.config_file) {
            Ok(contents) => {
                serde_yaml::from_str(&contents).map_err(|source| ConfigError::Parse {
                    path: paths.config_file.clone(),
                    source,
                })?
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => FileConfig::default(),
            Err(source) => {
                return Err(ConfigError::Read {
                    path: paths.config_file.clone(),
                    source,
                });
            }
        };
        Self::from_file(file_config, paths.model_cache.clone(), catalog)
    }

    fn from_file(
        file: FileConfig,
        default_cache: PathBuf,
        catalog: &ModelCatalog,
    ) -> Result<Self, ConfigError> {
        let cache_dir = file.cache_dir.unwrap_or(default_cache);
        let tts = TtsConfig::from_value(file.tts, cache_dir.clone(), catalog);
        let config = Self {
            provider: file.provider,
            model_id: file.model_id,
            accelerator: file.accelerator,
            language: file.language,
            cache_dir,
            max_audio_bytes: file.max_audio_bytes,
            max_audio_seconds: file.max_audio_seconds,
            queue_capacity: file.queue_capacity,
            log_level: file.log_level,
            tts,
        };
        config.validate(catalog)?;
        Ok(config)
    }

    fn validate(&self, catalog: &ModelCatalog) -> Result<(), ConfigError> {
        if self.model_id.trim().is_empty()
            || !self
                .model_id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        {
            return Err(ConfigError::Invalid(
                "model_id must be a non-empty identifier".into(),
            ));
        }
        if self.provider.trim().is_empty()
            || !self
                .provider
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        {
            return Err(ConfigError::Invalid(
                "provider must be a non-empty identifier".into(),
            ));
        }
        let metadata = catalog
            .model(&self.provider, &self.model_id)
            .ok_or_else(|| {
                ConfigError::Invalid(format!(
                    "unknown STT provider/model pair `{}/{}`",
                    self.provider, self.model_id
                ))
            })?;
        if !matches!(metadata.kind, LoaderKind::Parakeet | LoaderKind::Canary) {
            return Err(ConfigError::Invalid(format!(
                "registry kind `{:?}` is not supported for STT",
                metadata.kind
            )));
        }
        if self.language.trim().is_empty()
            || !self
                .language
                .chars()
                .all(|c| c.is_ascii_alphabetic() || c == '-')
        {
            return Err(ConfigError::Invalid(
                "language must be a non-empty language tag".into(),
            ));
        }
        if !metadata
            .languages
            .iter()
            .any(|language| language.as_str() == self.language)
        {
            return Err(ConfigError::Invalid(format!(
                "language `{}` is unsupported by `{}/{}`",
                self.language, self.provider, self.model_id
            )));
        }
        if self.max_audio_bytes == 0 || self.max_audio_bytes > MAX_AUDIO_BYTES {
            return Err(ConfigError::Invalid(format!(
                "max_audio_bytes must be between 1 and {MAX_AUDIO_BYTES}"
            )));
        }
        if self.max_audio_seconds == 0 || self.max_audio_seconds > MAX_AUDIO_SECONDS {
            return Err(ConfigError::Invalid(format!(
                "max_audio_seconds must be between 1 and {MAX_AUDIO_SECONDS}"
            )));
        }
        if self.queue_capacity == 0 || self.queue_capacity > MAX_QUEUE_CAPACITY {
            return Err(ConfigError::Invalid(format!(
                "queue_capacity must be between 1 and {MAX_QUEUE_CAPACITY}"
            )));
        }
        if self.cache_dir.as_os_str().is_empty() {
            return Err(ConfigError::Invalid("cache_dir must not be empty".into()));
        }
        match self.accelerator {
            Accelerator::Cuda if !cfg!(feature = "cuda") => {
                return Err(ConfigError::Invalid(
                    "CUDA is not compiled into this package".into(),
                ));
            }
            Accelerator::Migraphx if !cfg!(feature = "migraphx") => {
                return Err(ConfigError::Invalid(
                    "MIGraphX is not compiled into this package".into(),
                ));
            }
            _ => {}
        }
        Ok(())
    }
}

impl TtsConfig {
    fn from_value(
        value: Option<serde_yaml::Value>,
        default_cache: PathBuf,
        catalog: &ModelCatalog,
    ) -> Result<Self, String> {
        let file = match value {
            Some(value) => serde_yaml::from_value::<TtsFileConfig>(value)
                .map_err(|error| format!("invalid tts configuration: {error}"))?,
            None => TtsFileConfig::default(),
        };
        let model_id = file
            .model_id
            .unwrap_or_else(|| match file.provider.as_str() {
                DEFAULT_TTS_PROVIDER => DEFAULT_TTS_MODEL_ID.into(),
                "qwentts-cpp" => DEFAULT_QWEN_BASE_MODEL_ID.into(),
                _ => String::new(),
            });
        let kind = catalog
            .model(&file.provider, &model_id)
            .map(|metadata| metadata.kind)
            .ok_or_else(|| {
                format!(
                    "unsupported TTS provider/model combination `{}/{model_id}`",
                    file.provider
                )
            })?;
        match kind {
            LoaderKind::Kokoro
                if file.default_voice_description.is_some()
                    || file.default_clone_reference.is_some()
                    || file.default_clone_transcript.is_some()
                    || file.sampling.is_some() =>
            {
                let field = if file.default_voice_description.is_some() {
                    "default_voice_description"
                } else if file.default_clone_reference.is_some() {
                    "default_clone_reference"
                } else if file.default_clone_transcript.is_some() {
                    "default_clone_transcript"
                } else {
                    "sampling"
                };
                return Err(format!("tts.{field} is not valid for Kokoro"));
            }
            LoaderKind::Base
                if file.default_voice.is_some() || file.default_voice_description.is_some() =>
            {
                let field = if file.default_voice.is_some() {
                    "default_voice"
                } else {
                    "default_voice_description"
                };
                return Err(format!("tts.{field} is not valid for Qwen Base"));
            }
            LoaderKind::CustomVoice
                if file.default_voice_description.is_some()
                    || file.default_clone_reference.is_some()
                    || file.default_clone_transcript.is_some() =>
            {
                return Err("clone/design defaults are not valid for Qwen CustomVoice".into());
            }
            LoaderKind::VoiceDesign
                if file.default_voice.is_some()
                    || file.default_clone_reference.is_some()
                    || file.default_clone_transcript.is_some() =>
            {
                return Err("voice/clone defaults are not valid for Qwen VoiceDesign".into());
            }
            LoaderKind::Parakeet | LoaderKind::Canary => {
                return Err(format!("registry kind `{kind:?}` is not supported for TTS"));
            }
            _ => {}
        }
        let provider = match kind {
            LoaderKind::Kokoro => TtsProviderConfig::Kokoro {
                model_id,
                default_voice: file
                    .default_voice
                    .unwrap_or_else(|| DEFAULT_TTS_VOICE.into()),
            },
            LoaderKind::Base => TtsProviderConfig::QwenBase {
                model_id,
                default_clone_reference: file.default_clone_reference,
                default_clone_transcript: file.default_clone_transcript,
                sampling: file.sampling.unwrap_or_default(),
            },
            LoaderKind::CustomVoice => TtsProviderConfig::QwenCustomVoice {
                model_id,
                default_voice: file
                    .default_voice
                    .unwrap_or_else(|| DEFAULT_QWEN_CUSTOM_VOICE.into()),
                sampling: file.sampling.unwrap_or_default(),
            },
            LoaderKind::VoiceDesign => TtsProviderConfig::QwenVoiceDesign {
                model_id,
                default_voice_description: file
                    .default_voice_description
                    .unwrap_or_else(|| DEFAULT_QWEN_VOICE_DESCRIPTION.into())
                    .trim()
                    .to_owned(),
                sampling: file.sampling.unwrap_or_default(),
            },
            LoaderKind::Parakeet | LoaderKind::Canary => unreachable!(),
        };
        let config = Self {
            operational: TtsOperationalConfig {
                cache_dir: default_cache,
                default_speed: file.default_speed,
                pipewire_node: file.pipewire_node,
                volume: file.volume,
                max_text_bytes: file.max_text_bytes,
                max_reference_audio_bytes: file.max_reference_audio_bytes,
                max_reference_audio_seconds: file.max_reference_audio_seconds,
                max_generated_audio_seconds: file.max_generated_audio_seconds,
                queue_capacity: file.queue_capacity,
            },
            provider,
        };
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<(), String> {
        let operational = &self.operational;
        if !operational.cache_dir.is_absolute()
            || (operational.cache_dir.exists() && !operational.cache_dir.is_dir())
        {
            return Err("cache_dir must be an absolute directory path".into());
        }
        if let TtsProviderConfig::QwenBase {
            default_clone_reference,
            default_clone_transcript,
            ..
        } = &self.provider
        {
            if default_clone_transcript.is_some() && default_clone_reference.is_none() {
                return Err("tts.default_clone_transcript requires default_clone_reference".into());
            }
            if let Some(path) = default_clone_reference
                && (!path.is_absolute() || !path.is_file())
            {
                return Err("tts.default_clone_reference must be an existing absolute file".into());
            }
            if let Some(transcript) = default_clone_transcript {
                if transcript.trim().is_empty()
                    || transcript.len() as u64 > operational.max_text_bytes
                {
                    return Err(
                        "tts.default_clone_transcript must be nonempty and within max_text_bytes"
                            .into(),
                    );
                }
                if transcript.chars().any(|character| {
                    character == '\0' || (character.is_control() && !character.is_whitespace())
                }) {
                    return Err(
                        "tts.default_clone_transcript contains a disallowed control character"
                            .into(),
                    );
                }
            }
        }
        if let Some(default_voice) = self.default_voice()
            && (default_voice.trim().is_empty()
                || !default_voice.chars().all(|character| {
                    character.is_ascii_alphanumeric() || matches!(character, '_' | '-')
                }))
        {
            return Err("tts.default_voice must be a non-empty voice identifier".into());
        }
        if self.provider_id() == "qwentts-cpp" {
            if operational.default_speed != 1.0 {
                return Err("tts.default_speed must be 1.0 for Qwen providers".into());
            }
        } else if !operational.default_speed.is_finite()
            || !(MIN_TTS_SPEED..=MAX_TTS_SPEED).contains(&operational.default_speed)
        {
            return Err(format!(
                "tts.default_speed must be finite and between {MIN_TTS_SPEED} and {MAX_TTS_SPEED}"
            ));
        }
        if let Some(description) = self.default_voice_description() {
            if description.is_empty() {
                return Err("tts.default_voice_description must not be empty".into());
            }
            if description.len() as u64 > operational.max_text_bytes {
                return Err(format!(
                    "tts.default_voice_description exceeds max_text_bytes ({})",
                    operational.max_text_bytes
                ));
            }
            if description.chars().any(|character| {
                character == '\0' || (character.is_control() && !character.is_whitespace())
            }) {
                return Err(
                    "tts.default_voice_description contains a disallowed control character".into(),
                );
            }
        }
        if let Some(sampling) = self.sampling() {
            if sampling.seed.is_some_and(|seed| seed > i64::MAX as u64) {
                return Err("tts.sampling.seed must fit in a signed 64-bit integer".into());
            }
            if !(1..=MAX_QWEN_NEW_TOKENS).contains(&sampling.max_new_tokens) {
                return Err(format!(
                    "tts.sampling.max_new_tokens must be between 1 and {MAX_QWEN_NEW_TOKENS}"
                ));
            }
            if !sampling.temperature.is_finite() || !(0.01..=2.0).contains(&sampling.temperature) {
                return Err(
                    "tts.sampling.temperature must be finite and between 0.01 and 2.0".into(),
                );
            }
            if !(1..=MAX_QWEN_TOP_K).contains(&sampling.top_k) {
                return Err(format!(
                    "tts.sampling.top_k must be between 1 and {MAX_QWEN_TOP_K}"
                ));
            }
            if !sampling.top_p.is_finite() || !(0.01..=1.0).contains(&sampling.top_p) {
                return Err("tts.sampling.top_p must be finite and between 0.01 and 1.0".into());
            }
            if !sampling.repetition_penalty.is_finite()
                || !(0.5..=2.0).contains(&sampling.repetition_penalty)
            {
                return Err(
                    "tts.sampling.repetition_penalty must be finite and between 0.5 and 2.0".into(),
                );
            }
        }
        if let Some(node) = &operational.pipewire_node
            && (node.trim().is_empty() || node.chars().any(char::is_control))
        {
            return Err(
                "tts.pipewire_node must be a non-empty node name without control characters".into(),
            );
        }
        if !operational.volume.is_finite() || !(0.0..=1.0).contains(&operational.volume) {
            return Err("tts.volume must be finite and between 0.0 and 1.0".into());
        }
        if operational.max_text_bytes == 0 || operational.max_text_bytes > MAX_TEXT_BYTES {
            return Err(format!(
                "tts.max_text_bytes must be between 1 and {MAX_TEXT_BYTES}"
            ));
        }
        if operational.max_reference_audio_bytes == 0
            || operational.max_reference_audio_bytes > MAX_AUDIO_BYTES
        {
            return Err(format!(
                "tts.max_reference_audio_bytes must be between 1 and {MAX_AUDIO_BYTES}"
            ));
        }
        if operational.max_reference_audio_seconds == 0
            || operational.max_reference_audio_seconds > MAX_AUDIO_SECONDS
        {
            return Err(format!(
                "tts.max_reference_audio_seconds must be between 1 and {MAX_AUDIO_SECONDS}"
            ));
        }
        if operational.max_generated_audio_seconds == 0
            || operational.max_generated_audio_seconds > MAX_AUDIO_SECONDS
        {
            return Err(format!(
                "tts.max_generated_audio_seconds must be between 1 and {MAX_AUDIO_SECONDS}"
            ));
        }
        if operational.queue_capacity == 0 || operational.queue_capacity > MAX_QUEUE_CAPACITY {
            return Err(format!(
                "tts.queue_capacity must be between 1 and {MAX_QUEUE_CAPACITY}"
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog() -> ModelCatalog {
        ModelCatalog::from_yaml(include_str!("../model_registry.yaml")).unwrap()
    }

    fn fixture(contents: Option<&str>) -> (tempfile::TempDir, ConfigPaths) {
        let root = tempfile::tempdir().unwrap();
        let paths = ConfigPaths::from_homes(root.path().join("config"), root.path().join("cache"));
        if let Some(contents) = contents {
            fs::create_dir_all(paths.config_file.parent().unwrap()).unwrap();
            fs::write(&paths.config_file, contents).unwrap();
        }
        (root, paths)
    }

    #[test]
    fn defaults_select_registered_pairs_and_one_shared_cache() {
        let (_root, paths) = fixture(None);
        let config = Config::load_with_catalog(&paths, &catalog()).unwrap();
        assert_eq!(config.provider, DEFAULT_STT_PROVIDER);
        assert_eq!(config.model_id, DEFAULT_MODEL_ID);
        assert_eq!(config.cache_dir, paths.model_cache);
        let tts = config.tts.unwrap();
        assert_eq!(tts.provider_id(), DEFAULT_TTS_PROVIDER);
        assert_eq!(tts.model_id(), DEFAULT_TTS_MODEL_ID);
        assert_eq!(tts.operational.cache_dir, config.cache_dir);
    }

    #[test]
    fn supported_provider_model_and_limits_load() {
        let (_root, paths) = fixture(Some(
            "provider: transcribe-rs\nmodel_id: canary-180m-flash-en-es-de-fr-int8\naccelerator: cpu\nlanguage: de\ncache_dir: /tmp/sophon-models\nmax_audio_bytes: 1024\nmax_audio_seconds: 30\nqueue_capacity: 2\nlog_level: debug\n",
        ));
        let config = Config::load_with_catalog(&paths, &catalog()).unwrap();
        assert_eq!(config.language, "de");
        assert_eq!(config.cache_dir, PathBuf::from("/tmp/sophon-models"));
        assert_eq!(config.queue_capacity, 2);
    }

    #[test]
    fn removed_stt_and_tts_acquisition_fields_are_rejected() {
        for field in [
            "engine: parakeet",
            "quantization: int8",
            "translate: false",
            "model_path: /tmp/model",
            "automatic_download: true",
        ] {
            let (_root, paths) = fixture(Some(field));
            assert!(matches!(
                Config::load_with_catalog(&paths, &catalog()),
                Err(ConfigError::Parse { .. })
            ));
        }
        for field in [
            "model_path: /tmp/model",
            "cache_dir: /tmp/tts",
            "automatic_download: true",
        ] {
            let yaml = format!("tts:\n  {field}\n");
            let (_root, paths) = fixture(Some(&yaml));
            let config = Config::load_with_catalog(&paths, &catalog()).unwrap();
            assert!(config.tts.unwrap_err().contains("unknown field"));
        }
    }

    #[test]
    fn unknown_stt_pair_or_language_fails_strictly() {
        for yaml in [
            "provider: transcribe-rs\nmodel_id: missing\n",
            "provider: transcribe-rs\nmodel_id: parakeet-tdt-0.6b-v3-int8\nlanguage: ja\n",
            "provider: qwentts-cpp\nmodel_id: qwen3-tts-0.6b-base-q8_0\n",
        ] {
            let (_root, paths) = fixture(Some(yaml));
            assert!(matches!(
                Config::load_with_catalog(&paths, &catalog()),
                Err(ConfigError::Invalid(_))
            ));
        }
    }

    #[test]
    fn invalid_tts_is_isolated_from_valid_stt() {
        let (_root, paths) = fixture(Some("tts:\n  provider: qwentts-cpp\n  model_id: missing\n"));
        let config = Config::load_with_catalog(&paths, &catalog()).unwrap();
        assert_eq!(config.model_id, DEFAULT_MODEL_ID);
        assert!(config.tts.is_err());

        let (_root, paths) = fixture(Some(
            "tts:\n  provider: transcribe-rs\n  model_id: parakeet-tdt-0.6b-v3-int8\n",
        ));
        assert!(
            Config::load_with_catalog(&paths, &catalog())
                .unwrap()
                .tts
                .is_err()
        );
    }

    #[test]
    fn qwen_modes_apply_sane_defaults_and_reject_inapplicable_fields() {
        let cases = [
            ("qwen3-tts-0.6b-base-q8_0", LoaderKind::Base),
            ("qwen3-tts-0.6b-custom-voice-q8_0", LoaderKind::CustomVoice),
            ("qwen3-tts-1.7b-voice-design-q8_0", LoaderKind::VoiceDesign),
        ];
        for (model, kind) in cases {
            let yaml = format!("tts:\n  provider: qwentts-cpp\n  model_id: {model}\n");
            let (_root, paths) = fixture(Some(&yaml));
            let tts = Config::load_with_catalog(&paths, &catalog())
                .unwrap()
                .tts
                .unwrap();
            match kind {
                LoaderKind::Base => assert!(tts.default_clone().is_none()),
                LoaderKind::CustomVoice => {
                    assert_eq!(tts.default_voice(), Some(DEFAULT_QWEN_CUSTOM_VOICE))
                }
                LoaderKind::VoiceDesign => assert_eq!(
                    tts.default_voice_description(),
                    Some(DEFAULT_QWEN_VOICE_DESCRIPTION)
                ),
                _ => unreachable!(),
            }
        }
        let (_root, paths) = fixture(Some(
            "tts:\n  provider: qwentts-cpp\n  model_id: qwen3-tts-0.6b-custom-voice-q8_0\n  default_voice_description: warm\n",
        ));
        assert!(
            Config::load_with_catalog(&paths, &catalog())
                .unwrap()
                .tts
                .is_err()
        );
    }

    #[test]
    fn base_clone_defaults_are_startup_validated() {
        let root = tempfile::tempdir().unwrap();
        let reference = root.path().join("reference.wav");
        fs::write(&reference, b"fixture").unwrap();
        let paths = ConfigPaths::from_homes(root.path().join("config"), root.path().join("cache"));
        fs::create_dir_all(paths.config_file.parent().unwrap()).unwrap();
        fs::write(&paths.config_file, format!(
            "tts:\n  provider: qwentts-cpp\n  model_id: qwen3-tts-0.6b-base-q8_0\n  default_clone_reference: {}\n  default_clone_transcript: hello\n",
            reference.display(),
        )).unwrap();
        let config = Config::load_with_catalog(&paths, &catalog()).unwrap();
        let tts = config.tts.unwrap();
        let (path, transcript) = tts.default_clone().unwrap();
        assert_eq!(path, &reference);
        assert_eq!(transcript, Some("hello"));
    }
}
