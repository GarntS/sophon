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
    domain::SophonError,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelFile {
    pub relative_path: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
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

const PARAKEET_FILES: &[ModelFile] = &[
    ModelFile {
        relative_path: "encoder-model.int8.onnx",
        url: "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/resolve/8f23f0c03c8761650bdb5b40aaf3e40d2c15f1ce/encoder-model.int8.onnx",
        sha256: "6139d2fa7e1b086097b277c7149725edbab89cc7c7ae64b23c741be4055aff09",
    },
    ModelFile {
        relative_path: "decoder_joint-model.int8.onnx",
        url: "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/resolve/8f23f0c03c8761650bdb5b40aaf3e40d2c15f1ce/decoder_joint-model.int8.onnx",
        sha256: "eea7483ee3d1a30375daedc8ed83e3960c91b098812127a0d99d1c8977667a70",
    },
    ModelFile {
        relative_path: "nemo128.onnx",
        url: "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/resolve/8f23f0c03c8761650bdb5b40aaf3e40d2c15f1ce/nemo128.onnx",
        sha256: "a9fde1486ebfcc08f328d75ad4610c67835fea58c73ba57e3209a6f6cf019e9f",
    },
    ModelFile {
        relative_path: "vocab.txt",
        url: "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/resolve/8f23f0c03c8761650bdb5b40aaf3e40d2c15f1ce/vocab.txt",
        sha256: "d58544679ea4bc6ac563d1f545eb7d474bd6cfa467f0a6e2c1dc1c7d37e3c35d",
    },
];

const CANARY_FILES: &[ModelFile] = &[
    ModelFile {
        relative_path: "encoder-model.int8.onnx",
        url: "https://huggingface.co/istupakov/canary-180m-flash-onnx/resolve/92c2231a4e2b2524277fea759be967d2e6edfc49/encoder-model.int8.onnx",
        sha256: "996d1c89e6cbc891a7c88bf410884c178ffa474f7b13084522ac74a5e144cc81",
    },
    ModelFile {
        relative_path: "decoder-model.int8.onnx",
        url: "https://huggingface.co/istupakov/canary-180m-flash-onnx/resolve/92c2231a4e2b2524277fea759be967d2e6edfc49/decoder-model.int8.onnx",
        sha256: "9dd9c447872088c912e916d73751f9621a54085d5bc46788454fe904db51a914",
    },
    ModelFile {
        relative_path: "nemo128.onnx",
        url: "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/resolve/8f23f0c03c8761650bdb5b40aaf3e40d2c15f1ce/nemo128.onnx",
        sha256: "a9fde1486ebfcc08f328d75ad4610c67835fea58c73ba57e3209a6f6cf019e9f",
    },
    ModelFile {
        relative_path: "vocab.txt",
        url: "https://huggingface.co/istupakov/canary-180m-flash-onnx/resolve/92c2231a4e2b2524277fea759be967d2e6edfc49/vocab.txt",
        sha256: "2dae6fc7815f9640645e0c765522b278ee0cef49b482d91f6913e334628d3e77",
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

pub fn validate_layout(root: &Path, model: &ModelDefinition) -> Result<(), SophonError> {
    for file in model.files {
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

pub fn cache_path(cache_root: &Path, model: &ModelDefinition) -> PathBuf {
    cache_root.join(model.id).join(model.revision)
}

/// Returns a cache entry only after its complete manifest and expected layout
/// have been verified. Invalid or partial entries are never usable offline.
pub fn validated_cache(cache_root: &Path, model: &'static ModelDefinition) -> Option<PathBuf> {
    let root = cache_path(cache_root, model);
    for file in model.files {
        let mut input = File::open(root.join(file.relative_path)).ok()?;
        let mut hasher = Sha256::new();
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let read = input.read(&mut buffer).ok()?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
        if format!("{:x}", hasher.finalize()) != file.sha256 {
            return None;
        }
    }
    validate_layout(&root, model).ok()?;
    Some(root)
}

/// Acquires every manifest file into a temporary directory, verifies it, and
/// atomically publishes the complete model directory. `reqwest` honors the
/// standard proxy environment variables by default.
pub async fn acquire<F>(
    cache_root: &Path,
    model: &'static ModelDefinition,
    mut progress: F,
) -> Result<PathBuf, SophonError>
where
    F: FnMut(f32),
{
    if let Some(path) = validated_cache(cache_root, model) {
        return Ok(path);
    }
    fs::create_dir_all(cache_root).map_err(|e| SophonError::ModelUnavailable(e.to_string()))?;
    let lock_path = cache_root.join(format!(".{}.lock", model.id));
    // Advisory locking can block while another daemon downloads. Keep that wait
    // off the async executor so concurrent acquisitions do not deadlock it.
    let _lock = tokio::task::spawn_blocking(move || -> Result<File, SophonError> {
        let lock =
            File::create(lock_path).map_err(|e| SophonError::ModelUnavailable(e.to_string()))?;
        lock.lock_exclusive()
            .map_err(|e| SophonError::ModelUnavailable(e.to_string()))?;
        Ok(lock)
    })
    .await
    .map_err(|e| SophonError::ModelUnavailable(format!("model-cache lock task failed: {e}")))??;
    if let Some(path) = validated_cache(cache_root, model) {
        return Ok(path);
    }

    let temporary = tempfile::Builder::new()
        .prefix(".sophon-download-")
        .tempdir_in(cache_root)
        .map_err(|e| SophonError::ModelUnavailable(e.to_string()))?;
    let client = reqwest::Client::new();
    let total = model.files.len() as f32;
    for (index, file) in model.files.iter().enumerate() {
        let response = client
            .get(file.url)
            .send()
            .await
            .map_err(|e| SophonError::ModelUnavailable(format!("download {}: {e}", file.url)))?
            .error_for_status()
            .map_err(|e| SophonError::ModelUnavailable(format!("download {}: {e}", file.url)))?;
        let destination = temporary.path().join(file.relative_path);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).map_err(|e| SophonError::ModelUnavailable(e.to_string()))?;
        }
        let mut output =
            File::create(&destination).map_err(|e| SophonError::ModelUnavailable(e.to_string()))?;
        let mut stream = response.bytes_stream();
        let mut hasher = Sha256::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| {
                SophonError::ModelUnavailable(format!("download {}: {e}", file.url))
            })?;
            hasher.update(&chunk);
            output
                .write_all(&chunk)
                .map_err(|e| SophonError::ModelUnavailable(e.to_string()))?;
        }
        if format!("{:x}", hasher.finalize()) != file.sha256 {
            return Err(SophonError::ModelUnavailable(format!(
                "SHA-256 mismatch for {}",
                file.relative_path
            )));
        }
        progress((index + 1) as f32 / total);
    }
    validate_layout(temporary.path(), model)?;
    let destination = cache_path(cache_root, model);
    if destination.exists() {
        fs::remove_dir_all(&destination)
            .map_err(|e| SophonError::ModelUnavailable(e.to_string()))?;
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|e| SophonError::ModelUnavailable(e.to_string()))?;
    }
    fs::rename(temporary.path(), &destination)
        .map_err(|e| SophonError::ModelUnavailable(e.to_string()))?;
    Ok(destination)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_has_pinned_https_files_for_both_engines() {
        for model in [PARAKEET, CANARY] {
            assert!(
                model
                    .files
                    .iter()
                    .all(|file| file.url.starts_with("https://"))
            );
            assert!(model.files.iter().all(|file| file.sha256.len() == 64));
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
    fn partial_cache_is_not_usable() {
        let cache = tempfile::tempdir().unwrap();
        let root = cache_path(cache.path(), &PARAKEET);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join(PARAKEET.files[0].relative_path), "partial").unwrap();
        assert_eq!(validated_cache(cache.path(), &PARAKEET), None);
    }

    fn fixture_model(url: String, bytes: &[u8]) -> &'static ModelDefinition {
        let file = ModelFile {
            relative_path: "model.onnx",
            url: Box::leak(url.into_boxed_str()),
            sha256: Box::leak(format!("{:x}", Sha256::digest(bytes)).into_boxed_str()),
        };
        ModelDefinition {
            id: "fixture-model",
            engine: Engine::Parakeet,
            revision: "fixture",
            quantization: Quantization::Int8,
            files: Box::leak(vec![file].into_boxed_slice()),
            capabilities: ModelCapabilities {
                languages: &["en"],
                translation_to_english: false,
            },
        }
        .leak()
    }

    trait Leak {
        fn leak(self) -> &'static ModelDefinition;
    }
    impl Leak for ModelDefinition {
        fn leak(self) -> &'static ModelDefinition {
            Box::leak(Box::new(self))
        }
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
    async fn download_is_verified_published_and_reused_offline() {
        let bytes = b"verified fixture";
        let (url, requests) = fixture_server(bytes.to_vec());
        let model = fixture_model(url, bytes);
        let cache = tempfile::tempdir().unwrap();
        let path = acquire(cache.path(), model, |_| {}).await.unwrap();
        assert_eq!(std::fs::read(path.join("model.onnx")).unwrap(), bytes);
        assert_eq!(acquire(cache.path(), model, |_| {}).await.unwrap(), path);
        assert_eq!(requests.load(std::sync::atomic::Ordering::SeqCst), 1);
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
        let model = fixture_model(url, b"expected bytes");
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
        let model = fixture_model(interrupted_fixture_server(), bytes);
        let cache = tempfile::tempdir().unwrap();
        assert!(matches!(
            acquire(cache.path(), model, |_| {}).await,
            Err(SophonError::ModelUnavailable(_))
        ));
        assert!(!cache_path(cache.path(), model).exists());

        let (url, _) = fixture_server(bytes.to_vec());
        let recovered = fixture_model(url, bytes);
        assert!(acquire(cache.path(), recovered, |_| {}).await.is_ok());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn concurrent_acquisition_publishes_one_verified_cache_entry() {
        let bytes = b"concurrent fixture";
        let (url, requests) = fixture_server(bytes.to_vec());
        let model = fixture_model(url, bytes);
        let cache = tempfile::tempdir().unwrap();
        let (first, second) = tokio::join!(
            acquire(cache.path(), model, |_| {}),
            acquire(cache.path(), model, |_| {}),
        );
        assert_eq!(first.unwrap(), second.unwrap());
        assert_eq!(requests.load(std::sync::atomic::Ordering::SeqCst), 1);
    }
}
