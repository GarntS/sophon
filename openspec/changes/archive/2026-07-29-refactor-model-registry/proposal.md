## Why

Model metadata, artifact acquisition, cache validation, and observable runtime lifecycle are currently entangled across `acquisition.rs`, `lifecycle.rs`, configuration, and provider startup. A single package-defined model registry will make model files a shared, provider-neutral resource while leaving STT and TTS providers responsible for reporting their own runtime state.

## What Changes

- Replace the static in-code acquisition catalog with one process-global `ModelRegistry` initialized from a package-installed, read-only `model_registry.yaml`.
- Define provider/model manifests in YAML with a provider ID, model ID, loader `kind`, supported languages, revision metadata, and semantic role names for every required model file and sidecar.
- Store and verify files as shared content-addressed blobs, assemble model-specific hard-linked views, download missing files automatically, and return a `HashMap` from semantic file role to verified path.
- Deduplicate concurrent requests for the same artifact and make an acquisition or verification failure terminal for that model until daemon restart.
- Replace mutable lifecycle controllers with a dumb `ModelState` enum; long-lived STT and TTS provider handles report `Initializing`, registry-backed `Downloading`, provider-owned `Loading` and `Ready`, or terminal `Failed` state.
- Treat `(provider, model)` as model identity; revisions describe manifests and cache content but are not part of identity.
- Make provider/model `kind` select the concrete loader, including separate Qwen Base, CustomVoice, and VoiceDesign model entries under `qwentts-cpp`.
- **BREAKING** Remove transcription translation configuration, request options, and capability handling because translation is unsupported.
- **BREAKING** Remove local model-path and automatic-download policy from user configuration; missing registered artifacts are always downloaded.
- Add startup-validated, mode-specific user defaults for TTS cloning, named voices, and voice-design prompts, with request-level values taking precedence.
- Do not reload the package registry or user configuration during a daemon process lifetime.

## Capabilities

### New Capabilities
- `model-registry`: Package manifest loading, singleton registry behavior, provider/model lookup, verified shared artifacts, model views, path resolution, acquisition status, and terminal failures.
- `provider-model-state`: Long-lived STT/TTS provider handles that derive observable model state from registry acquisition status and provider runtime status.

### Modified Capabilities
- `shared-model-artifacts`: Route shared blob verification, locking, progress, and publication through the singleton registry and make failures terminal for the daemon lifetime.
- `transcription-models`: Select STT loaders from registry provider/model metadata and remove local override and download-policy behavior.
- `synthesis-models`: Select TTS loaders from registry metadata and consume role-keyed verified paths.
- `service-configuration`: Remove acquisition policy, local model overrides, STT engine/translation settings, and add provider/model selection plus mode-specific TTS defaults.
- `speech-transcription`: Remove the `translate` request option and translation behavior.
- `qwen-tts-providers`: Apply configured Base, CustomVoice, and VoiceDesign defaults while preserving request-level precedence.

## Impact

- Replaces `src/acquisition.rs` and the mutable state in `src/lifecycle.rs` with registry and provider-handle modules.
- Changes STT/TTS provider construction, daemon startup, D-Bus state reporting, configuration types, and model-resolution APIs.
- Adds a Serde-deserialized package data file and Nix/package wiring for its immutable installed path.
- Migrates curated STT, Kokoro, and Qwen manifests from Rust constants to YAML while preserving pinned URLs, sizes, digests, revisions, shared-blob caching, and assembled model directories.
- Requires updates to unit tests, D-Bus integration tests, package tests, and configuration documentation.
