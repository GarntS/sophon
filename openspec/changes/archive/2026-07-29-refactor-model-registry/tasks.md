## 1. Registry Schema and Package Catalog

- [x] 1.1 Define strict Serde types for provider/model manifests, loader kinds, supported languages, revisions, semantic file roles, and file identities.
- [x] 1.2 Implement startup validation for identifiers, supported kinds, nonempty manifests, safe relative paths, HTTPS URLs, exact SHA-256 values, and nonzero sizes.
- [x] 1.3 Create `model_registry.yaml` containing parity-checked Parakeet, Canary, Kokoro, and all five curated Qwen model definitions with named roles and shared codec identity.
- [x] 1.4 Add schema fixtures and tests covering valid catalogs, malformed YAML, unknown fields/kinds, unsafe paths, invalid identities, and duplicate or empty entries.
- [x] 1.5 Install the read-only registry through Nix/package outputs and expose its deterministic production path to daemon startup.

## 2. Singleton Registry and Artifact Resolution

- [x] 2.1 Implement explicitly initialized process-global `ModelRegistry` access while retaining directly constructible isolated instances for tests.
- [x] 2.2 Implement provider/model lookup, composite identity, metadata access, and unknown-pair errors without network activity.
- [x] 2.3 Migrate content-addressed blob validation, per-digest cross-process locking, streamed download, byte progress, size/digest verification, and atomic publication from acquisition code.
- [x] 2.4 Implement one shared asynchronous resolution attempt per model, memoized successful role maps, and terminal process-lifetime failure without retry.
- [x] 2.5 Implement atomic hard-linked model views and return complete `HashMap<String, PathBuf>` values keyed by semantic role.
- [x] 2.6 Add tests for concurrent callers, terminal failure, restart retry through a fresh registry, shared blobs, partial success reuse, corrupt cache replacement, progress, and changed revisions under stable identity.

## 3. Provider-Owned State

- [x] 3.1 Replace mutable lifecycle controllers and snapshots with the data-only `ModelState` enum and any transport-facing immutable state values still required.
- [x] 3.2 Introduce a long-lived STT provider handle that exists before initialization, derives acquisition state from the registry, and owns loading, ready, worker, and terminal failure state.
- [x] 3.3 Introduce the corresponding long-lived TTS provider handle, including runtime voices and capabilities after successful loading.
- [x] 3.4 Update D-Bus state properties and change notifications to read independent STT/TTS provider handles.
- [x] 3.5 Add state-transition tests for initializing, downloading progress, loading, ready, registry failure, native load failure, and independent STT/TTS outcomes.

## 4. STT Registry Integration

- [x] 4.1 Replace STT engine/model acquisition with provider/model registry resolution and select Parakeet or Canary from registry `kind`.
- [x] 4.2 Validate the complete required STT role set before deriving the assembled model directory and invoking `transcribe-rs`.
- [x] 4.3 Source supported-language validation and active provider/model reporting from registry metadata.
- [x] 4.4 Remove translation from STT options, backend capabilities, configuration, D-Bus request decoding, tests, and documentation.
- [x] 4.5 Add cached, uncached, unknown-model, missing-role, unsupported-kind, language, and terminal-failure STT tests.

## 5. TTS Registry Integration and Defaults

- [x] 5.1 Replace Kokoro and Qwen acquisition with provider/model registry resolution and select Kokoro, Base, CustomVoice, or VoiceDesign from registry `kind`.
- [x] 5.2 Validate Kokoro model/voices and Qwen talker/codec semantic roles before invoking native loaders.
- [x] 5.3 Source TTS supported-language validation and active provider/model reporting from registry metadata while retaining runtime-discovered voices and capabilities.
- [x] 5.4 Add startup-validated Base clone reference/transcript, CustomVoice named voice, and VoiceDesign prompt defaults with documented sane fallbacks.
- [x] 5.5 Apply valid request-level clone, named voice, and prompt values ahead of configured defaults for that request without mutating daemon configuration.
- [x] 5.6 Add mode-specific default, request precedence, role validation, shared-codec, unknown-model, and terminal-failure TTS tests.

## 6. Configuration and Daemon Composition

- [x] 6.1 Replace STT engine selection with provider/model selection and remove quantization, translation, model-path, and automatic-download configuration fields.
- [x] 6.2 Remove TTS model-path, cache override, and automatic-download fields and validate remaining fields against the selected registry kind.
- [x] 6.3 Retain one shared XDG/configured registry cache root and update documented startup defaults.
- [x] 6.4 Initialize the package registry once before creating provider handles and refactor daemon startup around asynchronous handle initialization.
- [x] 6.5 Preserve strict isolation where invalid TTS configuration or initialization does not invalidate otherwise valid STT startup.
- [x] 6.6 Update configuration tests and examples for removed fields, provider/model pairs, shared cache behavior, and mode-specific defaults.

## 7. Remove Legacy Acquisition Architecture

- [x] 7.1 Remove curated Rust manifest constants and all consumers after YAML parity tests pass.
- [x] 7.2 Delete `acquisition.rs`, obsolete location/override types, acquisition APIs, and mutable lifecycle storage.
- [x] 7.3 Remove compatibility exports and update module documentation, imports, public API references, and test fixtures to the registry/provider-handle architecture.

## 8. End-to-End Validation

- [x] 8.1 Update the isolated D-Bus integration test for provider-owned states, registry progress/failure, language-only STT options, and TTS default precedence.
- [x] 8.2 Add package tests proving the installed registry is present, read-only, parseable, and contains exact curated artifact metadata.
- [x] 8.3 Run formatting, static checks, unit tests, integration tests, offline verified-cache tests, provider smoke tests, and relevant Nix package checks.
- [x] 8.4 Update user-facing configuration and model-cache documentation, including breaking-field migration and restart-only retry semantics.
