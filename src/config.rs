//! Startup-only configuration loading, XDG path discovery, and validation.

use std::{env, fs, path::PathBuf};

use directories::BaseDirs;
use serde::Deserialize;
use thiserror::Error;

pub const DEFAULT_MODEL_ID: &str = "parakeet-tdt-0.6b-v3-int8";
pub const DEFAULT_MAX_AUDIO_BYTES: u64 = 32 * 1024 * 1024;
pub const DEFAULT_MAX_AUDIO_SECONDS: u64 = 10 * 60;
pub const DEFAULT_QUEUE_CAPACITY: usize = 8;
pub const DEFAULT_TTS_PROVIDER: &str = "tts-rs";
pub const DEFAULT_TTS_MODEL_ID: &str = "kokoro-v1.0-int8";
pub const DEFAULT_TTS_VOICE: &str = "af_heart";
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

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct TtsFileConfig {
    provider: String,
    model_id: String,
    model_path: Option<PathBuf>,
    cache_dir: Option<PathBuf>,
    automatic_download: bool,
    default_voice: String,
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
            model_id: DEFAULT_TTS_MODEL_ID.into(),
            model_path: None,
            cache_dir: None,
            automatic_download: true,
            default_voice: DEFAULT_TTS_VOICE.into(),
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
pub struct TtsConfig {
    pub provider: String,
    pub model_id: String,
    pub model_path: Option<PathBuf>,
    pub cache_dir: PathBuf,
    pub automatic_download: bool,
    pub default_voice: String,
    pub default_speed: f64,
    pub pipewire_node: Option<String>,
    pub volume: f64,
    pub max_text_bytes: u64,
    pub max_reference_audio_bytes: u64,
    pub max_reference_audio_seconds: u64,
    pub max_generated_audio_seconds: u64,
    pub queue_capacity: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct FileConfig {
    engine: Engine,
    model_id: String,
    model_path: Option<PathBuf>,
    quantization: Quantization,
    accelerator: Accelerator,
    language: String,
    translate: bool,
    cache_dir: Option<PathBuf>,
    automatic_download: bool,
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
            engine: Engine::Parakeet,
            model_id: DEFAULT_MODEL_ID.into(),
            model_path: None,
            quantization: Quantization::Int8,
            accelerator: Accelerator::Auto,
            language: "en".into(),
            translate: false,
            cache_dir: None,
            automatic_download: true,
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
    pub engine: Engine,
    pub model_id: String,
    pub model_path: Option<PathBuf>,
    pub quantization: Quantization,
    pub accelerator: Accelerator,
    pub language: String,
    pub translate: bool,
    pub cache_dir: PathBuf,
    pub automatic_download: bool,
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
        Self::from_file(file_config, paths.model_cache.clone())
    }

    fn from_file(file: FileConfig, default_cache: PathBuf) -> Result<Self, ConfigError> {
        let tts = TtsConfig::from_value(file.tts, default_cache.join("tts"));
        let config = Self {
            engine: file.engine,
            model_id: file.model_id,
            model_path: file.model_path,
            quantization: file.quantization,
            accelerator: file.accelerator,
            language: file.language,
            translate: file.translate,
            cache_dir: file.cache_dir.unwrap_or(default_cache),
            automatic_download: file.automatic_download,
            max_audio_bytes: file.max_audio_bytes,
            max_audio_seconds: file.max_audio_seconds,
            queue_capacity: file.queue_capacity,
            log_level: file.log_level,
            tts,
        };
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<(), ConfigError> {
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
        let engine_prefix = match self.engine {
            Engine::Parakeet => "parakeet",
            Engine::Canary => "canary",
        };
        if !self.model_id.starts_with(engine_prefix) {
            return Err(ConfigError::Invalid(format!(
                "model_id `{}` is not valid for {engine_prefix}",
                self.model_id
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
        if let Some(path) = &self.model_path
            && (!path.is_absolute() || !path.is_dir())
        {
            return Err(ConfigError::Invalid(
                "model_path must be an existing absolute directory".into(),
            ));
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
    ) -> Result<Self, String> {
        let file = match value {
            Some(value) => serde_yaml::from_value::<TtsFileConfig>(value)
                .map_err(|error| format!("invalid tts configuration: {error}"))?,
            None => TtsFileConfig::default(),
        };
        let config = Self {
            provider: file.provider,
            model_id: file.model_id,
            model_path: file.model_path,
            cache_dir: file.cache_dir.unwrap_or(default_cache),
            automatic_download: file.automatic_download,
            default_voice: file.default_voice,
            default_speed: file.default_speed,
            pipewire_node: file.pipewire_node,
            volume: file.volume,
            max_text_bytes: file.max_text_bytes,
            max_reference_audio_bytes: file.max_reference_audio_bytes,
            max_reference_audio_seconds: file.max_reference_audio_seconds,
            max_generated_audio_seconds: file.max_generated_audio_seconds,
            queue_capacity: file.queue_capacity,
        };
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<(), String> {
        if self.provider != DEFAULT_TTS_PROVIDER || self.model_id != DEFAULT_TTS_MODEL_ID {
            return Err(format!(
                "unsupported TTS provider/model combination `{}/{}`",
                self.provider, self.model_id
            ));
        }
        if let Some(path) = &self.model_path
            && (!path.is_absolute() || !path.is_dir())
        {
            return Err("tts.model_path must be an existing absolute directory".into());
        }
        if !self.cache_dir.is_absolute() || (self.cache_dir.exists() && !self.cache_dir.is_dir()) {
            return Err("tts.cache_dir must be an absolute directory path".into());
        }
        if self.default_voice.trim().is_empty()
            || !self.default_voice.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '_' | '-')
            })
        {
            return Err("tts.default_voice must be a non-empty voice identifier".into());
        }
        if !self.default_speed.is_finite()
            || !(MIN_TTS_SPEED..=MAX_TTS_SPEED).contains(&self.default_speed)
        {
            return Err(format!(
                "tts.default_speed must be finite and between {MIN_TTS_SPEED} and {MAX_TTS_SPEED}"
            ));
        }
        if let Some(node) = &self.pipewire_node
            && (node.trim().is_empty() || node.chars().any(char::is_control))
        {
            return Err(
                "tts.pipewire_node must be a non-empty node name without control characters".into(),
            );
        }
        if !self.volume.is_finite() || !(0.0..=1.0).contains(&self.volume) {
            return Err("tts.volume must be finite and between 0.0 and 1.0".into());
        }
        if self.max_text_bytes == 0 || self.max_text_bytes > MAX_TEXT_BYTES {
            return Err(format!(
                "tts.max_text_bytes must be between 1 and {MAX_TEXT_BYTES}"
            ));
        }
        if self.max_reference_audio_bytes == 0 || self.max_reference_audio_bytes > MAX_AUDIO_BYTES {
            return Err(format!(
                "tts.max_reference_audio_bytes must be between 1 and {MAX_AUDIO_BYTES}"
            ));
        }
        if self.max_reference_audio_seconds == 0
            || self.max_reference_audio_seconds > MAX_AUDIO_SECONDS
        {
            return Err(format!(
                "tts.max_reference_audio_seconds must be between 1 and {MAX_AUDIO_SECONDS}"
            ));
        }
        if self.max_generated_audio_seconds == 0
            || self.max_generated_audio_seconds > MAX_AUDIO_SECONDS
        {
            return Err(format!(
                "tts.max_generated_audio_seconds must be between 1 and {MAX_AUDIO_SECONDS}"
            ));
        }
        if self.queue_capacity == 0 || self.queue_capacity > MAX_QUEUE_CAPACITY {
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
    use std::path::Path;
    use tempfile::tempdir;

    fn paths(root: &Path) -> ConfigPaths {
        ConfigPaths::from_homes(root.join("config"), root.join("cache"))
    }

    #[test]
    fn uses_documented_defaults_when_file_is_absent() {
        let temp = tempdir().unwrap();
        let config = Config::load(&paths(temp.path())).unwrap();
        assert_eq!(config.model_id, DEFAULT_MODEL_ID);
        assert_eq!(config.max_audio_bytes, DEFAULT_MAX_AUDIO_BYTES);
        assert_eq!(config.max_audio_seconds, DEFAULT_MAX_AUDIO_SECONDS);
        assert_eq!(config.queue_capacity, DEFAULT_QUEUE_CAPACITY);
        let tts = config.tts.unwrap();
        assert_eq!(tts.provider, DEFAULT_TTS_PROVIDER);
        assert_eq!(tts.model_id, DEFAULT_TTS_MODEL_ID);
        assert_eq!(tts.default_voice, DEFAULT_TTS_VOICE);
        assert_eq!(tts.default_speed, DEFAULT_TTS_SPEED);
        assert_eq!(tts.pipewire_node, None);
        assert_eq!(tts.volume, DEFAULT_TTS_VOLUME);
        assert_eq!(tts.max_text_bytes, DEFAULT_MAX_TEXT_BYTES);
        assert_eq!(
            tts.max_reference_audio_bytes,
            DEFAULT_MAX_REFERENCE_AUDIO_BYTES
        );
        assert_eq!(
            tts.max_reference_audio_seconds,
            DEFAULT_MAX_REFERENCE_AUDIO_SECONDS
        );
        assert_eq!(
            tts.max_generated_audio_seconds,
            DEFAULT_MAX_GENERATED_AUDIO_SECONDS
        );
        assert_eq!(tts.queue_capacity, DEFAULT_TTS_QUEUE_CAPACITY);
        assert_eq!(tts.cache_dir, paths(temp.path()).model_cache.join("tts"));
    }

    #[test]
    fn loads_complete_configuration() {
        let temp = tempdir().unwrap();
        let paths = paths(temp.path());
        fs::create_dir_all(paths.config_file.parent().unwrap()).unwrap();
        let model_path = temp.path().join("model");
        fs::create_dir(&model_path).unwrap();
        fs::write(&paths.config_file, format!(
            "engine: parakeet\nmodel_id: parakeet-custom\nmodel_path: {}\nquantization: fp16\naccelerator: cpu\nlanguage: de\ntranslate: true\ncache_dir: {}\nautomatic_download: false\nmax_audio_bytes: 1024\nmax_audio_seconds: 30\nqueue_capacity: 2\nlog_level: debug\n",
            model_path.display(), temp.path().join("cache-override").display()
        )).unwrap();
        let config = Config::load(&paths).unwrap();
        assert_eq!(config.language, "de");
        assert!(!config.automatic_download);
        assert_eq!(config.queue_capacity, 2);
    }

    #[test]
    fn merges_partial_configuration_and_rejects_unknown_fields() {
        let temp = tempdir().unwrap();
        let paths = paths(temp.path());
        fs::create_dir_all(paths.config_file.parent().unwrap()).unwrap();
        fs::write(&paths.config_file, "language: fr\n").unwrap();
        assert_eq!(Config::load(&paths).unwrap().language, "fr");
        fs::write(&paths.config_file, "unknown: true\n").unwrap();
        assert!(matches!(
            Config::load(&paths),
            Err(ConfigError::Parse { .. })
        ));
        fs::write(&paths.config_file, "engine: [\n").unwrap();
        assert!(matches!(
            Config::load(&paths),
            Err(ConfigError::Parse { .. })
        ));
    }

    #[test]
    fn loads_partial_and_complete_tts_configuration() {
        let temp = tempdir().unwrap();
        let paths = paths(temp.path());
        fs::create_dir_all(paths.config_file.parent().unwrap()).unwrap();

        fs::write(&paths.config_file, "tts:\n  default_voice: bf_emma\n").unwrap();
        let partial = Config::load(&paths).unwrap().tts.unwrap();
        assert_eq!(partial.default_voice, "bf_emma");
        assert_eq!(partial.default_speed, DEFAULT_TTS_SPEED);
        assert_eq!(partial.max_text_bytes, DEFAULT_MAX_TEXT_BYTES);

        let model_path = temp.path().join("kokoro");
        fs::create_dir(&model_path).unwrap();
        let cache_path = temp.path().join("tts-cache");
        fs::write(
            &paths.config_file,
            format!(
                "tts:\n  provider: tts-rs\n  model_id: kokoro-v1.0-int8\n  model_path: {}\n  cache_dir: {}\n  automatic_download: false\n  default_voice: am_adam\n  default_speed: 1.25\n  pipewire_node: alsa_output.fixture\n  volume: 0.5\n  max_text_bytes: 4096\n  max_reference_audio_bytes: 1048576\n  max_reference_audio_seconds: 20\n  max_generated_audio_seconds: 120\n  queue_capacity: 3\n",
                model_path.display(),
                cache_path.display()
            ),
        )
        .unwrap();
        let complete = Config::load(&paths).unwrap().tts.unwrap();
        assert_eq!(complete.model_path.as_deref(), Some(model_path.as_path()));
        assert_eq!(complete.cache_dir, cache_path);
        assert!(!complete.automatic_download);
        assert_eq!(complete.default_speed, 1.25);
        assert_eq!(
            complete.pipewire_node.as_deref(),
            Some("alsa_output.fixture")
        );
        assert_eq!(complete.volume, 0.5);
        assert_eq!(complete.queue_capacity, 3);
    }

    #[test]
    fn localizes_unknown_and_malformed_tts_fields_to_tts() {
        let temp = tempdir().unwrap();
        let paths = paths(temp.path());
        fs::create_dir_all(paths.config_file.parent().unwrap()).unwrap();

        for tts in [
            "tts:\n  unknown: true\n",
            "tts:\n  volume: loud\n",
            "tts:\n  queue_capacity: []\n",
        ] {
            fs::write(&paths.config_file, format!("language: fr\n{tts}")).unwrap();
            let config = Config::load(&paths).unwrap();
            assert_eq!(config.language, "fr");
            assert!(config.tts.is_err());
        }
    }

    #[test]
    fn rejects_invalid_tts_combinations_paths_strings_ranges_and_limits() {
        let temp = tempdir().unwrap();
        let paths = paths(temp.path());
        fs::create_dir_all(paths.config_file.parent().unwrap()).unwrap();

        let invalid_mappings = [
            "provider: qwentts-cpp",
            "model_id: kokoro-unknown",
            "model_path: relative/model",
            "cache_dir: relative/cache",
            "default_voice: ''",
            "default_voice: 'bad voice'",
            "default_speed: .nan",
            "default_speed: 0.1",
            "pipewire_node: ''",
            "volume: .inf",
            "volume: 1.1",
            "max_text_bytes: 0",
            "max_reference_audio_bytes: 0",
            "max_reference_audio_seconds: 0",
            "max_generated_audio_seconds: 0",
            "queue_capacity: 0",
            "queue_capacity: 129",
        ];
        for mapping in invalid_mappings {
            fs::write(
                &paths.config_file,
                format!("language: de\ntts:\n  {mapping}\n"),
            )
            .unwrap();
            let config = Config::load(&paths).unwrap();
            assert_eq!(config.language, "de", "mapping: {mapping}");
            assert!(config.tts.is_err(), "mapping should fail: {mapping}");
        }
    }

    #[test]
    fn rejects_inconsistent_and_out_of_range_values() {
        let temp = tempdir().unwrap();
        let paths = paths(temp.path());
        fs::create_dir_all(paths.config_file.parent().unwrap()).unwrap();
        fs::write(
            &paths.config_file,
            "engine: canary\nmodel_id: parakeet-tdt-0.6b-v3-int8\n",
        )
        .unwrap();
        assert!(matches!(Config::load(&paths), Err(ConfigError::Invalid(_))));
        fs::write(&paths.config_file, "max_audio_bytes: 0\n").unwrap();
        assert!(matches!(Config::load(&paths), Err(ConfigError::Invalid(_))));
    }

    #[test]
    fn migraphx_is_accepted_only_when_compiled_in() {
        let temp = tempdir().unwrap();
        let paths = paths(temp.path());
        fs::create_dir_all(paths.config_file.parent().unwrap()).unwrap();
        fs::write(&paths.config_file, "accelerator: migraphx\n").unwrap();

        let result = Config::load(&paths);
        if cfg!(feature = "migraphx") {
            assert_eq!(result.unwrap().accelerator, Accelerator::Migraphx);
        } else {
            assert!(
                matches!(result, Err(ConfigError::Invalid(message)) if message.contains("MIGraphX"))
            );
        }
    }

    #[test]
    fn rejects_obsolete_rocm_accelerator_without_an_alias() {
        let temp = tempdir().unwrap();
        let paths = paths(temp.path());
        fs::create_dir_all(paths.config_file.parent().unwrap()).unwrap();
        fs::write(&paths.config_file, "accelerator: rocm\n").unwrap();

        assert!(matches!(
            Config::load(&paths),
            Err(ConfigError::Parse { source, .. })
                if source.to_string().contains("unknown variant `rocm`")
        ));
    }
}
