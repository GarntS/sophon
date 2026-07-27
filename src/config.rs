//! Startup-only configuration loading, XDG path discovery, and validation.

use std::{env, fs, path::PathBuf};

use directories::BaseDirs;
use serde::Deserialize;
use thiserror::Error;

pub const DEFAULT_MODEL_ID: &str = "parakeet-tdt-0.6b-v3-int8";
pub const DEFAULT_MAX_AUDIO_BYTES: u64 = 32 * 1024 * 1024;
pub const DEFAULT_MAX_AUDIO_SECONDS: u64 = 10 * 60;
pub const DEFAULT_QUEUE_CAPACITY: usize = 8;
const MAX_AUDIO_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_AUDIO_SECONDS: u64 = 60 * 60;
const MAX_QUEUE_CAPACITY: usize = 128;

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
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
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
