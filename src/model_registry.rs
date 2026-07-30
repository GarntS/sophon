//! Strict package model catalog types and validation.

use std::{
    collections::{BTreeMap, HashMap},
    env,
    fs::{self, File},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
};

use fs2::FileExt;
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use tokio::sync::Notify;

pub const PACKAGE_REGISTRY_ENV: &str = "SOPHON_MODEL_REGISTRY_PATH";

/// Returns the package registry selected by the launcher, with a source-tree
/// fallback for development and tests.
pub fn package_registry_path() -> PathBuf {
    env::var_os(PACKAGE_REGISTRY_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("model_registry.yaml"))
}

use serde::{
    Deserialize, Deserializer,
    de::{MapAccess, Visitor},
};
use thiserror::Error;

/// The immutable package catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelCatalog {
    pub providers: BTreeMap<String, BTreeMap<String, ModelManifest>>,
}

/// Metadata and required files for one provider/model identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelManifest {
    pub kind: LoaderKind,
    pub revision: String,
    pub languages: Vec<Language>,
    pub files: BTreeMap<String, ModelFile>,
}

/// Loader adapter selected by package metadata.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum LoaderKind {
    Parakeet,
    Canary,
    Kokoro,
    Base,
    CustomVoice,
    VoiceDesign,
}

impl LoaderKind {
    /// The exact semantic file-role keys a manifest of this kind must contain.
    /// `ModelCatalog::validate` enforces this set during package-catalog loading,
    /// before any resolution attempt is created; `initialize`/`initialize_tts`
    /// re-check it after resolution as a consumer-boundary defense.
    pub const fn required_roles(self) -> &'static [&'static str] {
        match self {
            Self::Parakeet => &["encoder", "decoder_joint", "nemo", "vocabulary"],
            Self::Canary => &["encoder", "decoder", "nemo", "vocabulary"],
            Self::Kokoro => &["model", "voices"],
            Self::Base | Self::CustomVoice | Self::VoiceDesign => &["talker", "codec"],
        }
    }
}

/// Languages understood by the curated providers.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    En,
    Zh,
    Ja,
    Ko,
    De,
    Fr,
    Ru,
    Pt,
    Es,
    It,
    Hi,
}

impl Language {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::Zh => "zh",
            Self::Ja => "ja",
            Self::Ko => "ko",
            Self::De => "de",
            Self::Fr => "fr",
            Self::Ru => "ru",
            Self::Pt => "pt",
            Self::Es => "es",
            Self::It => "it",
            Self::Hi => "hi",
        }
    }
}

/// One mandatory semantic file role in a model manifest.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ModelFile {
    pub path: PathBuf,
    pub url: String,
    pub sha256: String,
    pub size: u64,
}

/// Content identity shared by files with the same verified bytes.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileIdentity {
    pub sha256: String,
    pub size: u64,
}

struct UniqueMap<K, V>(BTreeMap<K, V>);

impl<'de, K, V> Deserialize<'de> for UniqueMap<K, V>
where
    K: Deserialize<'de> + Ord + std::fmt::Display,
    V: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct UniqueMapVisitor<K, V>(std::marker::PhantomData<(K, V)>);

        impl<'de, K, V> Visitor<'de> for UniqueMapVisitor<K, V>
        where
            K: Deserialize<'de> + Ord + std::fmt::Display,
            V: Deserialize<'de>,
        {
            type Value = UniqueMap<K, V>;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a mapping with unique keys")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut values = BTreeMap::new();
                while let Some((key, value)) = map.next_entry()? {
                    if values.insert(key, value).is_some() {
                        return Err(serde::de::Error::custom("duplicate mapping key"));
                    }
                }
                Ok(UniqueMap(values))
            }
        }

        deserializer.deserialize_map(UniqueMapVisitor(std::marker::PhantomData))
    }
}

impl<'de> Deserialize<'de> for ModelCatalog {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawCatalog {
            providers: UniqueMap<String, UniqueMap<String, ModelManifest>>,
        }
        let raw = RawCatalog::deserialize(deserializer)?;
        Ok(Self {
            providers: raw
                .providers
                .0
                .into_iter()
                .map(|(provider, models)| (provider, models.0))
                .collect(),
        })
    }
}

impl<'de> Deserialize<'de> for ModelManifest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawManifest {
            kind: LoaderKind,
            revision: String,
            languages: Vec<Language>,
            files: UniqueMap<String, ModelFile>,
        }
        let raw = RawManifest::deserialize(deserializer)?;
        Ok(Self {
            kind: raw.kind,
            revision: raw.revision,
            languages: raw.languages,
            files: raw.files.0,
        })
    }
}

impl ModelFile {
    pub fn identity(&self) -> FileIdentity {
        FileIdentity {
            sha256: self.sha256.clone(),
            size: self.size,
        }
    }
}

#[derive(Debug, Error)]
pub enum RegistryError {
    #[error("could not read model registry `{path}`: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("invalid model registry YAML: {0}")]
    Parse(#[from] serde_yaml::Error),
    #[error("invalid model registry: {0}")]
    Invalid(String),
    #[error("model registry resolution failed: {0}")]
    Resolution(String),
    #[error("the process-global model registry is not initialized")]
    GlobalUninitialized,
    #[error("the process-global model registry was already initialized")]
    GlobalAlreadyInitialized,
}

impl ModelCatalog {
    /// Loads and fully validates a startup-only package catalog.
    pub fn load(path: &Path) -> Result<Self, RegistryError> {
        let yaml = fs::read_to_string(path).map_err(|source| RegistryError::Read {
            path: path.to_owned(),
            source,
        })?;
        Self::from_yaml(&yaml)
    }

    pub fn from_yaml(yaml: &str) -> Result<Self, RegistryError> {
        let catalog: Self = serde_yaml::from_str(yaml)?;
        catalog.validate()?;
        Ok(catalog)
    }

    pub fn validate(&self) -> Result<(), RegistryError> {
        if self.providers.is_empty() {
            return Err(RegistryError::Invalid("providers must not be empty".into()));
        }
        for (provider, models) in &self.providers {
            validate_identifier(provider, "provider")?;
            if models.is_empty() {
                return Err(RegistryError::Invalid(format!(
                    "provider `{provider}` must contain at least one model"
                )));
            }
            for (model, manifest) in models {
                validate_identifier(model, "model")?;
                validate_revision(&manifest.revision, provider, model)?;
                if manifest.languages.is_empty() {
                    return Err(RegistryError::Invalid(format!(
                        "model `{provider}/{model}` must support at least one language"
                    )));
                }
                let mut languages = manifest.languages.clone();
                languages.sort_unstable();
                if languages.windows(2).any(|pair| pair[0] == pair[1]) {
                    return Err(RegistryError::Invalid(format!(
                        "model `{provider}/{model}` contains a duplicate language"
                    )));
                }
                if manifest.files.is_empty() {
                    return Err(RegistryError::Invalid(format!(
                        "model `{provider}/{model}` must contain at least one file"
                    )));
                }
                let mut paths = std::collections::BTreeSet::new();
                for (role, file) in &manifest.files {
                    validate_identifier(role, "file role")?;
                    validate_file(provider, model, role, file)?;
                    if !paths.insert(&file.path) {
                        return Err(RegistryError::Invalid(format!(
                            "model `{provider}/{model}` contains duplicate path `{}`",
                            file.path.display()
                        )));
                    }
                }
                let expected: std::collections::BTreeSet<&str> =
                    manifest.kind.required_roles().iter().copied().collect();
                let actual: std::collections::BTreeSet<&str> =
                    manifest.files.keys().map(String::as_str).collect();
                if actual != expected {
                    return Err(RegistryError::Invalid(format!(
                        "model `{provider}/{model}` has roles {actual:?}; expected {expected:?} for kind `{:?}`",
                        manifest.kind
                    )));
                }
            }
        }
        Ok(())
    }

    pub fn model(&self, provider: &str, model: &str) -> Option<&ModelManifest> {
        self.providers.get(provider)?.get(model)
    }
}

fn validate_identifier(value: &str, label: &str) -> Result<(), RegistryError> {
    let valid = !value.is_empty()
        && value.len() <= 128
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'-' | b'_')
        })
        && value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && value
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric);
    if valid {
        Ok(())
    } else {
        Err(RegistryError::Invalid(format!(
            "invalid {label} identifier `{value}`"
        )))
    }
}

fn validate_revision(revision: &str, provider: &str, model: &str) -> Result<(), RegistryError> {
    let valid = !revision.is_empty()
        && revision.len() <= 128
        && revision
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'));
    if valid {
        Ok(())
    } else {
        Err(RegistryError::Invalid(format!(
            "model `{provider}/{model}` has invalid revision `{revision}`"
        )))
    }
}

fn validate_file(
    provider: &str,
    model: &str,
    role: &str,
    file: &ModelFile,
) -> Result<(), RegistryError> {
    let prefix = format!("file `{provider}/{model}:{role}`");
    if file.path.as_os_str().is_empty()
        || file.path.is_absolute()
        || file
            .path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(RegistryError::Invalid(format!(
            "{prefix} has unsafe relative path `{}`",
            file.path.display()
        )));
    }
    let url = reqwest::Url::parse(&file.url)
        .map_err(|error| RegistryError::Invalid(format!("{prefix} has invalid URL: {error}")))?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(RegistryError::Invalid(format!(
            "{prefix} URL must be an HTTPS URL without credentials"
        )));
    }
    if file.sha256.len() != 64
        || !file
            .sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(RegistryError::Invalid(format!(
            "{prefix} has invalid SHA-256 digest"
        )));
    }
    if file.size == 0 {
        return Err(RegistryError::Invalid(format!(
            "{prefix} size must be nonzero"
        )));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ModelKey {
    pub provider: String,
    pub model: String,
}

impl ModelKey {
    pub fn new(provider: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            provider: provider.into(),
            model: model.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ResolutionStatus {
    Pending,
    Downloading { progress: f32 },
    Ready,
    Failed { message: String },
}

enum AttemptState {
    Pending,
    Running,
    Ready(Arc<HashMap<String, PathBuf>>),
    Failed(String),
}

struct ResolutionAttempt {
    state: Mutex<AttemptState>,
    progress_bits: std::sync::atomic::AtomicU32,
    changed: Notify,
}

impl ResolutionAttempt {
    fn new() -> Self {
        Self {
            state: Mutex::new(AttemptState::Pending),
            progress_bits: std::sync::atomic::AtomicU32::new(0),
            changed: Notify::new(),
        }
    }

    fn set_progress(&self, progress: f32) {
        self.progress_bits.store(
            progress.clamp(0.0, 1.0).to_bits(),
            std::sync::atomic::Ordering::Relaxed,
        );
        self.changed.notify_waiters();
    }

    fn progress(&self) -> f32 {
        f32::from_bits(
            self.progress_bits
                .load(std::sync::atomic::Ordering::Relaxed),
        )
    }
}

/// Startup-only catalog plus shared artifact resolution state.
pub struct ModelRegistry {
    catalog: ModelCatalog,
    cache_root: PathBuf,
    client: reqwest::Client,
    attempts: Mutex<HashMap<ModelKey, Arc<ResolutionAttempt>>>,
}

static GLOBAL_REGISTRY: OnceLock<Arc<ModelRegistry>> = OnceLock::new();

impl ModelRegistry {
    /// Constructs an isolated registry. Tests should use this instead of the global.
    pub fn new(catalog: ModelCatalog, cache_root: PathBuf, client: reqwest::Client) -> Self {
        Self {
            catalog,
            cache_root,
            client,
            attempts: Mutex::new(HashMap::new()),
        }
    }

    pub fn from_path(
        path: &Path,
        cache_root: PathBuf,
        client: reqwest::Client,
    ) -> Result<Self, RegistryError> {
        Ok(Self::new(ModelCatalog::load(path)?, cache_root, client))
    }

    pub fn initialize_global(registry: Self) -> Result<Arc<Self>, RegistryError> {
        let registry = Arc::new(registry);
        GLOBAL_REGISTRY
            .set(Arc::clone(&registry))
            .map_err(|_| RegistryError::GlobalAlreadyInitialized)?;
        Ok(registry)
    }

    pub fn global() -> Result<Arc<Self>, RegistryError> {
        GLOBAL_REGISTRY
            .get()
            .cloned()
            .ok_or(RegistryError::GlobalUninitialized)
    }

    pub fn catalog(&self) -> &ModelCatalog {
        &self.catalog
    }

    pub fn metadata(&self, provider: &str, model: &str) -> Result<&ModelManifest, RegistryError> {
        self.catalog.model(provider, model).ok_or_else(|| {
            RegistryError::Resolution(format!("unknown provider/model pair `{provider}/{model}`"))
        })
    }

    pub fn status(&self, provider: &str, model: &str) -> ResolutionStatus {
        let key = ModelKey::new(provider, model);
        let attempt = self
            .attempts
            .lock()
            .expect("registry attempts lock poisoned")
            .get(&key)
            .cloned();
        let Some(attempt) = attempt else {
            return ResolutionStatus::Pending;
        };
        let state = attempt
            .state
            .lock()
            .expect("registry attempt lock poisoned");
        match &*state {
            AttemptState::Pending => ResolutionStatus::Pending,
            AttemptState::Running => ResolutionStatus::Downloading {
                progress: attempt.progress(),
            },
            AttemptState::Ready(_) => ResolutionStatus::Ready,
            AttemptState::Failed(message) => ResolutionStatus::Failed {
                message: message.clone(),
            },
        }
    }

    /// Resolves all mandatory files. One in-process attempt is shared and its
    /// success or failure is retained for this registry's lifetime.
    pub async fn resolve(
        &self,
        provider: &str,
        model: &str,
    ) -> Result<HashMap<String, PathBuf>, RegistryError> {
        let key = ModelKey::new(provider, model);
        let attempt = {
            let mut attempts = self
                .attempts
                .lock()
                .expect("registry attempts lock poisoned");
            Arc::clone(
                attempts
                    .entry(key)
                    .or_insert_with(|| Arc::new(ResolutionAttempt::new())),
            )
        };

        loop {
            let notified = attempt.changed.notified();
            let is_owner = {
                let mut state = attempt
                    .state
                    .lock()
                    .expect("registry attempt lock poisoned");
                match &*state {
                    AttemptState::Ready(paths) => return Ok(paths.as_ref().clone()),
                    AttemptState::Failed(message) => {
                        return Err(RegistryError::Resolution(message.clone()));
                    }
                    AttemptState::Pending => {
                        *state = AttemptState::Running;
                        true
                    }
                    AttemptState::Running => false,
                }
            };
            if is_owner {
                let result = match self.metadata(provider, model).cloned() {
                    Ok(manifest) => {
                        self.resolve_once(provider, model, &manifest, &attempt)
                            .await
                    }
                    Err(error) => Err(error),
                };
                let mut state = attempt
                    .state
                    .lock()
                    .expect("registry attempt lock poisoned");
                match result {
                    Ok(paths) => {
                        attempt.set_progress(1.0);
                        *state = AttemptState::Ready(Arc::new(paths.clone()));
                        attempt.changed.notify_waiters();
                        return Ok(paths);
                    }
                    Err(error) => {
                        let message = error.to_string();
                        *state = AttemptState::Failed(message.clone());
                        attempt.changed.notify_waiters();
                        return Err(RegistryError::Resolution(message));
                    }
                }
            }
            notified.await;
        }
    }

    async fn resolve_once(
        &self,
        provider: &str,
        model: &str,
        manifest: &ModelManifest,
        attempt: &ResolutionAttempt,
    ) -> Result<HashMap<String, PathBuf>, RegistryError> {
        fs::create_dir_all(&self.cache_root).map_err(resolution_io)?;
        let total = manifest.files.values().try_fold(0_u64, |total, file| {
            total.checked_add(file.size).ok_or_else(|| {
                RegistryError::Resolution(format!(
                    "manifest size overflow for `{provider}/{model}`"
                ))
            })
        })?;
        let mut completed = 0_u64;
        let mut blobs = BTreeMap::new();
        for (role, file) in &manifest.files {
            let blob = self.acquire_blob(file, completed, total, attempt).await?;
            completed += file.size;
            attempt.set_progress(completed as f32 / total as f32);
            blobs.insert(role.clone(), blob);
        }
        self.publish_view(provider, model, manifest, &blobs)
    }

    async fn acquire_blob(
        &self,
        file: &ModelFile,
        completed: u64,
        total: u64,
        attempt: &ResolutionAttempt,
    ) -> Result<PathBuf, RegistryError> {
        let destination = self.cache_root.join("artifacts").join(&file.sha256);
        if validate_blob(&destination, file).is_ok() {
            return Ok(destination);
        }
        let artifacts = self.cache_root.join("artifacts");
        fs::create_dir_all(&artifacts).map_err(resolution_io)?;
        let _lock = lock_file(artifacts.join(format!(".{}.lock", file.sha256))).await?;
        if validate_blob(&destination, file).is_ok() {
            return Ok(destination);
        }
        let mut temporary = tempfile::Builder::new()
            .prefix(".download-")
            .tempfile_in(&artifacts)
            .map_err(resolution_io)?;
        let response = self
            .client
            .get(&file.url)
            .send()
            .await
            .map_err(|error| RegistryError::Resolution(format!("download {}: {error}", file.url)))?
            .error_for_status()
            .map_err(|error| {
                RegistryError::Resolution(format!("download {}: {error}", file.url))
            })?;
        let mut stream = response.bytes_stream();
        let mut hasher = Sha256::new();
        let mut downloaded = 0_u64;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|error| {
                RegistryError::Resolution(format!("download {}: {error}", file.url))
            })?;
            downloaded = downloaded.checked_add(chunk.len() as u64).ok_or_else(|| {
                RegistryError::Resolution(format!("download size overflow for {}", file.url))
            })?;
            hasher.update(&chunk);
            temporary.write_all(&chunk).map_err(resolution_io)?;
            attempt.set_progress((completed + downloaded.min(file.size)) as f32 / total as f32);
        }
        temporary
            .flush()
            .and_then(|()| temporary.as_file().sync_all())
            .map_err(resolution_io)?;
        if downloaded != file.size {
            return Err(RegistryError::Resolution(format!(
                "size mismatch for `{}`: expected {}, downloaded {downloaded}",
                file.path.display(),
                file.size
            )));
        }
        if format!("{:x}", hasher.finalize()) != file.sha256 {
            return Err(RegistryError::Resolution(format!(
                "SHA-256 mismatch for `{}`",
                file.path.display()
            )));
        }
        if destination.exists() {
            fs::remove_file(&destination).map_err(resolution_io)?;
        }
        temporary
            .persist(&destination)
            .map_err(|error| resolution_io(error.error))?;
        validate_blob(&destination, file)?;
        Ok(destination)
    }

    fn publish_view(
        &self,
        provider: &str,
        model: &str,
        manifest: &ModelManifest,
        blobs: &BTreeMap<String, PathBuf>,
    ) -> Result<HashMap<String, PathBuf>, RegistryError> {
        let fingerprint = manifest_fingerprint(provider, model, manifest);
        let destination = self
            .cache_root
            .join("views")
            .join(provider)
            .join(model)
            .join(fingerprint);
        if validate_view(&destination, manifest).is_ok() {
            return Ok(role_paths(&destination, manifest));
        }
        let parent = destination.parent().expect("model view has a parent");
        fs::create_dir_all(parent).map_err(resolution_io)?;
        let view_lock = File::create(parent.join(".view.lock")).map_err(resolution_io)?;
        FileExt::lock_exclusive(&view_lock).map_err(resolution_io)?;
        if validate_view(&destination, manifest).is_ok() {
            return Ok(role_paths(&destination, manifest));
        }
        if destination.exists() {
            fs::remove_dir_all(&destination).map_err(resolution_io)?;
        }
        let temporary = tempfile::Builder::new()
            .prefix(".view-")
            .tempdir_in(parent)
            .map_err(resolution_io)?;
        for (role, file) in &manifest.files {
            let target = temporary.path().join(&file.path);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(resolution_io)?;
            }
            fs::hard_link(&blobs[role], target).map_err(resolution_io)?;
        }
        validate_view(temporary.path(), manifest)?;
        match fs::rename(temporary.path(), &destination) {
            Ok(()) => {}
            Err(_error)
                if destination.is_dir() && validate_view(&destination, manifest).is_ok() => {}
            Err(error) => return Err(resolution_io(error)),
        }
        Ok(role_paths(&destination, manifest))
    }
}

fn resolution_io(error: std::io::Error) -> RegistryError {
    RegistryError::Resolution(error.to_string())
}

async fn lock_file(path: PathBuf) -> Result<File, RegistryError> {
    tokio::task::spawn_blocking(move || {
        let lock = File::create(path).map_err(resolution_io)?;
        FileExt::lock_exclusive(&lock).map_err(resolution_io)?;
        Ok(lock)
    })
    .await
    .map_err(|error| RegistryError::Resolution(format!("artifact lock task failed: {error}")))?
}

fn validate_blob(path: &Path, file: &ModelFile) -> Result<(), RegistryError> {
    let metadata = fs::metadata(path).map_err(resolution_io)?;
    if !metadata.is_file() || metadata.len() != file.size {
        return Err(RegistryError::Resolution(format!(
            "invalid cached artifact `{}`",
            path.display()
        )));
    }
    let mut input = File::open(path).map_err(resolution_io)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = input.read(&mut buffer).map_err(resolution_io)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    if format!("{:x}", hasher.finalize()) != file.sha256 {
        return Err(RegistryError::Resolution(format!(
            "invalid cached artifact `{}`",
            path.display()
        )));
    }
    Ok(())
}

fn validate_view(root: &Path, manifest: &ModelManifest) -> Result<(), RegistryError> {
    for file in manifest.files.values() {
        validate_blob(&root.join(&file.path), file)?;
    }
    Ok(())
}

fn role_paths(root: &Path, manifest: &ModelManifest) -> HashMap<String, PathBuf> {
    manifest
        .files
        .iter()
        .map(|(role, file)| (role.clone(), root.join(&file.path)))
        .collect()
}

/// Requires an exact semantic role set before a native provider is invoked.
pub fn require_roles(
    paths: &HashMap<String, PathBuf>,
    required: &[&str],
    identity: &str,
) -> Result<(), RegistryError> {
    let actual = paths
        .keys()
        .map(String::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    let expected = required
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    if actual == expected {
        Ok(())
    } else {
        Err(RegistryError::Resolution(format!(
            "registry model `{identity}` has roles {actual:?}; expected {expected:?}"
        )))
    }
}

/// Returns the shared assembled directory for role paths used by directory loaders.
pub fn common_model_root(
    paths: &HashMap<String, PathBuf>,
    identity: &str,
) -> Result<PathBuf, RegistryError> {
    let mut parents = paths.values().map(|path| path.parent());
    let root = parents.next().flatten().ok_or_else(|| {
        RegistryError::Resolution(format!(
            "registry model `{identity}` has no assembled model directory"
        ))
    })?;
    if parents.any(|parent| parent != Some(root)) {
        return Err(RegistryError::Resolution(format!(
            "registry model `{identity}` files do not share one directory"
        )));
    }
    Ok(root.to_owned())
}

fn manifest_fingerprint(provider: &str, model: &str, manifest: &ModelManifest) -> String {
    let mut hasher = Sha256::new();
    hasher.update(provider);
    hasher.update([0]);
    hasher.update(model);
    hasher.update([0]);
    hasher.update(&manifest.revision);
    for (role, file) in &manifest.files {
        hasher.update([0]);
        hasher.update(role);
        hasher.update([0]);
        hasher.update(file.path.as_os_str().as_encoded_bytes());
        hasher.update([0]);
        hasher.update(&file.sha256);
        hasher.update(file.size.to_le_bytes());
    }
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = r#"
providers:
  test-provider:
    test-model:
      kind: parakeet
      revision: abc123
      languages: [en]
      files:
        encoder:
          path: encoder.onnx
          url: https://example.com/encoder.onnx
          sha256: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
          size: 42
        decoder_joint:
          path: decoder_joint.onnx
          url: https://example.com/decoder_joint.onnx
          sha256: bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb
          size: 43
        nemo:
          path: nemo.onnx
          url: https://example.com/nemo.onnx
          sha256: cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc
          size: 44
        vocabulary:
          path: vocab.txt
          url: https://example.com/vocab.txt
          sha256: dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd
          size: 45
"#;

    #[test]
    fn accepts_a_valid_strict_catalog() {
        let catalog = ModelCatalog::from_yaml(VALID).unwrap();
        assert_eq!(
            catalog.model("test-provider", "test-model").unwrap().kind,
            LoaderKind::Parakeet
        );
    }

    #[test]
    fn rejects_malformed_yaml_unknown_fields_and_kinds() {
        assert!(matches!(
            ModelCatalog::from_yaml("providers: ["),
            Err(RegistryError::Parse(_))
        ));
        assert!(matches!(
            ModelCatalog::from_yaml(
                &VALID.replace("revision: abc123", "surprise: true\n      revision: abc123")
            ),
            Err(RegistryError::Parse(_))
        ));
        assert!(matches!(
            ModelCatalog::from_yaml(&VALID.replace("parakeet", "whisper")),
            Err(RegistryError::Parse(_))
        ));
    }

    #[test]
    fn rejects_unsafe_paths_urls_digests_sizes_and_empty_entries() {
        for invalid in [
            VALID.replace("encoder.onnx", "../encoder.onnx"),
            VALID.replace("https://", "http://"),
            VALID.replace(&"a".repeat(64), "abc"),
            VALID.replace("size: 42", "size: 0"),
            VALID.replace("      files:\n        encoder:\n          path: encoder.onnx\n          url: https://example.com/encoder.onnx\n          sha256: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n          size: 42", "      files: {}"),
        ] {
            assert!(ModelCatalog::from_yaml(&invalid).is_err(), "accepted:\n{invalid}");
        }
    }

    #[test]
    fn rejects_duplicate_or_empty_entries() {
        let duplicate_role = VALID.replace("        encoder:\n", "        encoder:\n          path: first.onnx\n          url: https://example.com/first\n          sha256: bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\n          size: 1\n        encoder:\n");
        assert!(ModelCatalog::from_yaml(&duplicate_role).is_err());
        assert!(ModelCatalog::from_yaml("providers: {}").is_err());
        assert!(ModelCatalog::from_yaml(&VALID.replace("test-provider", "Bad Provider")).is_err());
    }

    #[test]
    fn packaged_catalog_is_valid_and_has_shared_qwen_codec() {
        let catalog = ModelCatalog::from_yaml(include_str!("../model_registry.yaml")).unwrap();
        assert_eq!(
            catalog.providers.values().map(BTreeMap::len).sum::<usize>(),
            8
        );
        let qwen = &catalog.providers["qwentts-cpp"];
        assert_eq!(qwen.len(), 5);
        let identities = qwen
            .values()
            .map(|model| model.files["codec"].identity())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(identities.len(), 1);
    }

    fn role_catalog_yaml(kind: &str, roles: &[&str]) -> String {
        use std::fmt::Write as _;
        let mut yaml = String::from("providers:\n  fixture:\n    model:\n");
        let _ = writeln!(yaml, "      kind: {kind}");
        let _ = writeln!(yaml, "      revision: abc123");
        let _ = writeln!(yaml, "      languages: [en]");
        let _ = writeln!(yaml, "      files:");
        for (index, role) in roles.iter().enumerate() {
            // Build a distinct, valid 64-character lowercase-hex digest per role.
            let mut digest = String::from("a").repeat(63);
            digest.push(char::from(b'0' + index as u8));
            let _ = writeln!(yaml, "        {role}:");
            let _ = writeln!(yaml, "          path: file{index}.bin");
            let _ = writeln!(yaml, "          url: https://example.com/file{index}.bin");
            let _ = writeln!(yaml, "          sha256: {digest}");
            let _ = writeln!(yaml, "          size: {}", index as u64 + 1);
        }
        yaml
    }

    #[test]
    fn catalog_enforces_exact_role_set_for_every_loader_kind() {
        let cases = [
            (
                LoaderKind::Parakeet,
                "parakeet",
                &["encoder", "decoder_joint", "nemo", "vocabulary"][..],
            ),
            (
                LoaderKind::Canary,
                "canary",
                &["encoder", "decoder", "nemo", "vocabulary"],
            ),
            (LoaderKind::Kokoro, "kokoro", &["model", "voices"]),
            (LoaderKind::Base, "base", &["talker", "codec"]),
            (
                LoaderKind::CustomVoice,
                "custom-voice",
                &["talker", "codec"],
            ),
            (
                LoaderKind::VoiceDesign,
                "voice-design",
                &["talker", "codec"],
            ),
        ];
        for (kind, kind_str, roles) in cases {
            // The exact accepted role set loads for the daemon lifetime.
            assert!(
                ModelCatalog::from_yaml(&role_catalog_yaml(kind_str, roles)).is_ok(),
                "exact roles for kind {kind:?} were rejected"
            );
            // Matching the loader's required_roles() is equivalent.
            assert_eq!(kind.required_roles(), roles);
            // Missing any one role fails before resolution/network work.
            for index in 0..roles.len() {
                let missing: Vec<&str> = roles
                    .iter()
                    .enumerate()
                    .filter(|(j, _)| *j != index)
                    .map(|(_, role)| *role)
                    .collect();
                let error =
                    ModelCatalog::from_yaml(&role_catalog_yaml(kind_str, &missing)).unwrap_err();
                assert!(
                    matches!(error, RegistryError::Invalid(_)),
                    "missing role {:?} for {kind:?} not rejected",
                    roles[index]
                );
            }
            // An extra role fails.
            let mut extra = roles.to_vec();
            extra.push("extra");
            assert!(
                ModelCatalog::from_yaml(&role_catalog_yaml(kind_str, &extra)).is_err(),
                "extra role for kind {kind:?} accepted"
            );
            // A cross-kind substituted role fails.
            let foreign = match kind {
                LoaderKind::Parakeet | LoaderKind::Canary => "model",
                _ => "encoder",
            };
            let mut substituted = roles.to_vec();
            *substituted.last_mut().unwrap() = foreign;
            assert!(
                ModelCatalog::from_yaml(&role_catalog_yaml(kind_str, &substituted)).is_err(),
                "cross-kind role substitution for {kind:?} accepted"
            );
        }
    }

    fn fixture_catalog(revision: &str, files: Vec<(&str, &str, String, &[u8])>) -> ModelCatalog {
        let files = files
            .into_iter()
            .map(|(role, path, url, bytes)| {
                (
                    role.to_owned(),
                    ModelFile {
                        path: PathBuf::from(path),
                        url,
                        sha256: format!("{:x}", Sha256::digest(bytes)),
                        size: bytes.len() as u64,
                    },
                )
            })
            .collect();
        ModelCatalog {
            providers: BTreeMap::from([(
                "fixture".into(),
                BTreeMap::from([(
                    "model".into(),
                    ModelManifest {
                        kind: LoaderKind::Parakeet,
                        revision: revision.into(),
                        languages: vec![Language::En],
                        files,
                    },
                )]),
            )]),
        }
    }

    fn fixture_server(body: Vec<u8>) -> (String, Arc<std::sync::atomic::AtomicUsize>) {
        use std::{
            io::{Read as _, Write as _},
            net::TcpListener,
            sync::atomic::{AtomicUsize, Ordering},
            thread,
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let requests = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&requests);
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
        (format!("http://{address}/artifact"), requests)
    }

    fn interrupted_server() -> (String, Arc<std::sync::atomic::AtomicUsize>) {
        use std::{
            io::{Read as _, Write as _},
            net::TcpListener,
            sync::atomic::{AtomicUsize, Ordering},
            thread,
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let requests = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&requests);
        thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut request = [0; 1024];
            let _ = socket.read(&mut request);
            count.fetch_add(1, Ordering::SeqCst);
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 99\r\nConnection: close\r\n\r\npartial",
                )
                .unwrap();
        });
        (format!("http://{address}/artifact"), requests)
    }

    #[test]
    fn provider_role_validation_rejects_missing_extra_or_split_roles() {
        let complete = HashMap::from([
            ("talker".into(), PathBuf::from("/view/talker.gguf")),
            ("codec".into(), PathBuf::from("/view/codec.gguf")),
        ]);
        assert!(require_roles(&complete, &["talker", "codec"], "fixture/qwen").is_ok());
        assert_eq!(
            common_model_root(&complete, "fixture/qwen").unwrap(),
            PathBuf::from("/view")
        );
        assert!(require_roles(&complete, &["talker", "codec", "config"], "fixture/qwen").is_err());
        assert!(require_roles(&complete, &["talker"], "fixture/qwen").is_err());
        let split = HashMap::from([
            ("model".into(), PathBuf::from("/first/model.onnx")),
            ("voices".into(), PathBuf::from("/second/voices.bin")),
        ]);
        assert!(common_model_root(&split, "fixture/kokoro").is_err());
    }

    #[test]
    fn isolated_lookup_uses_composite_identity_without_network() {
        let cache = tempfile::tempdir().unwrap();
        let catalog = fixture_catalog(
            "one",
            vec![(
                "model",
                "model.bin",
                "http://127.0.0.1:9/unused".into(),
                b"bytes",
            )],
        );
        let registry = ModelRegistry::new(catalog, cache.path().into(), reqwest::Client::new());
        assert_eq!(
            registry.metadata("fixture", "model").unwrap().revision,
            "one"
        );
        assert!(registry.metadata("other", "model").is_err());
        assert_eq!(
            registry.status("fixture", "model"),
            ResolutionStatus::Pending
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn concurrent_resolution_shares_one_attempt_and_publishes_hard_linked_roles() {
        use std::sync::atomic::Ordering;
        let bytes = b"shared concurrent bytes";
        let (url, requests) = fixture_server(bytes.to_vec());
        let cache = tempfile::tempdir().unwrap();
        let registry = Arc::new(ModelRegistry::new(
            fixture_catalog("one", vec![("encoder", "nested/model.bin", url, bytes)]),
            cache.path().into(),
            reqwest::Client::new(),
        ));
        let (first, second) = tokio::join!(
            registry.resolve("fixture", "model"),
            registry.resolve("fixture", "model")
        );
        let first = first.unwrap();
        assert_eq!(first, second.unwrap());
        assert_eq!(requests.load(Ordering::SeqCst), 1);
        assert_eq!(fs::read(&first["encoder"]).unwrap(), bytes);
        assert_eq!(registry.status("fixture", "model"), ResolutionStatus::Ready);
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let blob = cache
                .path()
                .join("artifacts")
                .join(format!("{:x}", Sha256::digest(bytes)));
            assert_eq!(
                fs::metadata(blob).unwrap().ino(),
                fs::metadata(&first["encoder"]).unwrap().ino()
            );
        }
    }

    #[tokio::test]
    async fn failure_is_terminal_but_a_fresh_registry_retries() {
        use std::sync::atomic::Ordering;
        let expected = b"complete bytes";
        let (bad_url, bad_requests) = interrupted_server();
        let cache = tempfile::tempdir().unwrap();
        let failed = ModelRegistry::new(
            fixture_catalog("one", vec![("model", "model.bin", bad_url, expected)]),
            cache.path().into(),
            reqwest::Client::new(),
        );
        assert!(failed.resolve("fixture", "model").await.is_err());
        assert!(failed.resolve("fixture", "model").await.is_err());
        assert_eq!(bad_requests.load(Ordering::SeqCst), 1);
        assert!(matches!(
            failed.status("fixture", "model"),
            ResolutionStatus::Failed { .. }
        ));

        let (good_url, good_requests) = fixture_server(expected.to_vec());
        let fresh = ModelRegistry::new(
            fixture_catalog("one", vec![("model", "model.bin", good_url, expected)]),
            cache.path().into(),
            reqwest::Client::new(),
        );
        assert!(fresh.resolve("fixture", "model").await.is_ok());
        assert_eq!(good_requests.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn fresh_registry_replaces_corrupt_blob_and_changed_revision_view() {
        use std::sync::atomic::Ordering;
        let cache = tempfile::tempdir().unwrap();
        let first_bytes = b"first revision";
        let (first_url, _) = fixture_server(first_bytes.to_vec());
        let first = ModelRegistry::new(
            fixture_catalog("one", vec![("model", "model.bin", first_url, first_bytes)]),
            cache.path().into(),
            reqwest::Client::new(),
        )
        .resolve("fixture", "model")
        .await
        .unwrap();

        let digest = format!("{:x}", Sha256::digest(first_bytes));
        fs::write(
            cache.path().join("artifacts").join(digest),
            b"corrupt bytes!",
        )
        .unwrap();
        let (repair_url, repair_requests) = fixture_server(first_bytes.to_vec());
        let repaired = ModelRegistry::new(
            fixture_catalog("one", vec![("model", "model.bin", repair_url, first_bytes)]),
            cache.path().into(),
            reqwest::Client::new(),
        )
        .resolve("fixture", "model")
        .await
        .unwrap();
        assert_eq!(repair_requests.load(Ordering::SeqCst), 1);
        assert_eq!(fs::read(&repaired["model"]).unwrap(), first_bytes);

        let second_bytes = b"second revision";
        let (second_url, _) = fixture_server(second_bytes.to_vec());
        let second = ModelRegistry::new(
            fixture_catalog(
                "two",
                vec![("model", "model.bin", second_url, second_bytes)],
            ),
            cache.path().into(),
            reqwest::Client::new(),
        )
        .resolve("fixture", "model")
        .await
        .unwrap();
        assert_ne!(first["model"], second["model"]);
        assert_eq!(fs::read(&second["model"]).unwrap(), second_bytes);
    }

    #[tokio::test]
    async fn models_share_blobs_and_partial_success_survives_a_failed_model() {
        use std::sync::atomic::Ordering;
        let cache = tempfile::tempdir().unwrap();
        let shared = b"shared codec bytes";
        let missing = b"eventual talker bytes";
        let (shared_url, shared_requests) = fixture_server(shared.to_vec());
        let (bad_url, _) = interrupted_server();
        let failed = ModelRegistry::new(
            fixture_catalog(
                "one",
                vec![
                    ("a_codec", "codec.bin", shared_url, shared),
                    ("b_talker", "talker.bin", bad_url, missing),
                ],
            ),
            cache.path().into(),
            reqwest::Client::new(),
        );
        assert!(failed.resolve("fixture", "model").await.is_err());
        assert_eq!(shared_requests.load(Ordering::SeqCst), 1);

        let (talker_url, talker_requests) = fixture_server(missing.to_vec());
        let fresh = ModelRegistry::new(
            fixture_catalog(
                "one",
                vec![
                    (
                        "a_codec",
                        "codec.bin",
                        "http://127.0.0.1:9/must-not-download".into(),
                        shared,
                    ),
                    ("b_talker", "talker.bin", talker_url, missing),
                ],
            ),
            cache.path().into(),
            reqwest::Client::new(),
        );
        let paths = fresh.resolve("fixture", "model").await.unwrap();
        assert_eq!(talker_requests.load(Ordering::SeqCst), 1);
        assert_eq!(fs::read(&paths["a_codec"]).unwrap(), shared);

        let mut catalog = fixture_catalog(
            "one",
            vec![(
                "codec",
                "other-codec.bin",
                "http://127.0.0.1:9/must-not-download".into(),
                shared,
            )],
        );
        let original = catalog.providers["fixture"]["model"].clone();
        catalog
            .providers
            .get_mut("fixture")
            .unwrap()
            .insert("other-model".into(), original);
        let shared_registry =
            ModelRegistry::new(catalog, cache.path().into(), reqwest::Client::new());
        assert!(
            shared_registry
                .resolve("fixture", "other-model")
                .await
                .is_ok()
        );
    }

    fn chunked_server(first: Vec<u8>, second: Vec<u8>) -> String {
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
            thread::sleep(Duration::from_millis(100));
            socket.write_all(&second).unwrap();
        });
        format!("http://{address}/artifact")
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn status_reports_in_flight_byte_progress() {
        let first = vec![b'a'; 32 * 1024];
        let second = vec![b'b'; 32 * 1024];
        let bytes = [first.as_slice(), second.as_slice()].concat();
        let url = chunked_server(first, second);
        let cache = tempfile::tempdir().unwrap();
        let registry = Arc::new(ModelRegistry::new(
            fixture_catalog("one", vec![("model", "model.bin", url, &bytes)]),
            cache.path().into(),
            reqwest::Client::new(),
        ));
        let resolving = {
            let registry = Arc::clone(&registry);
            tokio::spawn(async move { registry.resolve("fixture", "model").await })
        };
        let mut observed_partial = false;
        for _ in 0..100 {
            if let ResolutionStatus::Downloading { progress } = registry.status("fixture", "model")
            {
                assert!((0.0..=1.0).contains(&progress));
                observed_partial |= progress > 0.0 && progress < 1.0;
            }
            if observed_partial {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert!(observed_partial);
        resolving.await.unwrap().unwrap();
        assert_eq!(registry.status("fixture", "model"), ResolutionStatus::Ready);
    }

    #[tokio::test]
    async fn unknown_resolution_becomes_terminal_without_a_network_request() {
        let cache = tempfile::tempdir().unwrap();
        let registry = ModelRegistry::new(
            fixture_catalog(
                "one",
                vec![(
                    "model",
                    "model.bin",
                    "http://127.0.0.1:9/unused".into(),
                    b"bytes",
                )],
            ),
            cache.path().into(),
            reqwest::Client::new(),
        );
        assert!(registry.resolve("fixture", "missing").await.is_err());
        assert!(matches!(
            registry.status("fixture", "missing"),
            ResolutionStatus::Failed { .. }
        ));
    }
}
