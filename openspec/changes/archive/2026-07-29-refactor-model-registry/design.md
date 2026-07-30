## Context

Sophon currently compiles curated model manifests into `acquisition.rs`, keeps separate STT and TTS lifecycle snapshots, and lets daemon startup coordinate cache checks, downloads, provider construction, and D-Bus state. This duplicates model-management policy across STT and TTS and couples observable state to mutable lifecycle controllers. The refactor must preserve verified shared blobs, concurrent acquisition safety, progress reporting, model-specific sidecars, package reproducibility, and independent STT/TTS failure behavior.

The package will install an immutable `model_registry.yaml`; user `config.yaml` selects provider/model pairs and runtime defaults but cannot redefine model artifacts. Configuration and registry data are both startup-only. Translation is removed rather than represented as permanently unsupported behavior.

## Goals / Non-Goals

**Goals:**
- Establish one process-global registry as the authority for model manifests, artifact availability, downloads, verification, and resolved paths.
- Load strict provider/model/file definitions from package YAML using Serde.
- Preserve content-addressed shared blobs and assembled model views.
- Give consumers role-keyed verified paths and require consumers to validate their required roles.
- Deduplicate concurrent acquisition and retain terminal per-model failure for the process lifetime.
- Make STT/TTS provider handles the authority for externally observable model state.
- Support mode-specific user defaults while letting request options override them.

**Non-Goals:**
- Runtime registry reload, user-defined registry entries, arbitrary local model overrides, offline-only operation, translation, or dynamic model switching.
- Making revision part of public model identity.
- Moving inference runtime ownership or worker queues into the registry.

## Decisions

### Package registry schema

The installed YAML is a strict mapping with unknown fields rejected:

```yaml
providers:
  transcribe-rs:
    parakeet-tdt-0.6b-v3-int8:
      kind: parakeet
      revision: 8f23f0c03c8761650bdb5b40aaf3e40d2c15f1ce
      languages: [en]
      files:
        encoder:
          path: encoder-model.int8.onnx
          url: https://example.invalid/encoder-model.int8.onnx
          sha256: 6139...
          size: 652183999
  qwentts-cpp:
    qwen3-tts-0.6b-base-q8_0:
      kind: base
      revision: e0f336a048a3de02b29b8ad92969217d9ecffe3e
      languages: [en, zh, ja, ko, de, fr, ru, pt, es, it]
      files:
        talker: { path: qwen-talker.gguf, url: https://..., sha256: ..., size: 992615488 }
        codec: { path: qwen-tokenizer.gguf, url: https://..., sha256: ..., size: 291150624 }
```

Provider and model are the composite identity. `revision` is descriptive manifest/cache metadata, not identity. `kind` is interpreted by the selected provider adapter. File-map keys are stable semantic roles; `path` is the validated relative filename used in the assembled view. All files in a manifest are mandatory.

A strict typed schema is chosen over arbitrary metadata so package errors fail at initialization rather than later inside native loaders. Model capabilities contain supported languages; translation metadata is absent.

### Singleton initialization and access

`ModelRegistry` is initialized exactly once during daemon startup with the package manifest path, XDG-derived shared cache root, and HTTP client, then exposed as an `Arc` through a process-global one-time cell. Tests construct isolated registry instances directly; they do not compete for the production global.

The package build supplies the immutable manifest path, including a Nix store path where applicable. A missing, unreadable, or invalid package manifest prevents provider initialization and produces terminal failed provider state.

### Registry API and resolved resources

The asynchronous resolution operation takes `(provider, model)` and returns `HashMap<String, PathBuf>`, keyed by semantic role. Unknown provider/model pairs fail without network access. Resolution always downloads missing or invalid required artifacts; there is no request policy or local override.

The registry guarantees that every returned path is a regular file matching the declared size and SHA-256 and that all manifest entries are present. Consumers still check required role names before invoking native loaders, producing a provider/model error when roles are missing or unexpected.

### Blob cache and assembled views

Artifacts remain content-addressed by SHA-256 and are independently locked, streamed to temporary files, size/digest verified, flushed, and atomically published. Identical identities share one blob across providers and models.

After all blobs verify, the registry atomically publishes a model view containing hard links under declared relative paths. Returned role paths point into that view, allowing directory-based loaders to use their common parent while role-based loaders consume individual paths. A changed package manifest for the same identity is reconciled on the next daemon start by validating and replacing its view; unchanged valid blobs are reused.

### Attempt and failure semantics

Within one daemon process each model has one acquisition attempt. Concurrent callers share that attempt and observe the same byte-weighted progress. Success is memoized as resolved paths; any registry lookup, I/O, network, size, digest, or publication failure is memoized as terminal `Failed` for that model until process restart. Completed valid blobs survive a model failure and may be reused by another model or the next daemon process.

Cross-process artifact locks remain necessary even though the registry is a process singleton.

### Provider-owned model state

`ModelState` is a data-only enum:

```rust
enum ModelState {
    Initializing,
    Downloading { progress: f32 },
    Loading,
    Ready,
    Failed { message: String },
}
```

A long-lived STT or TTS provider handle exists before asynchronous initialization begins. Its state accessor derives state in this order:
1. Before initialization starts: `Initializing`.
2. While waiting for registry resolution: registry `Downloading` progress or terminal registry failure.
3. After artifacts resolve and native construction runs: provider-owned `Loading`.
4. After its worker is usable: provider-owned `Ready`.
5. After native load failure: terminal provider-owned `Failed`.

The registry tracks only artifact attempt status; it does not claim that a model is loaded or inference-ready. D-Bus properties query provider handles, preserving independent STT and TTS state.

### Provider loading and language support

STT configuration selects provider/model; registry `kind` chooses Parakeet or Canary construction, replacing the separate engine selector. TTS registry entries similarly use kinds for Kokoro, Qwen Base, Qwen CustomVoice, and Qwen VoiceDesign. Providers validate requested language against registry metadata before queueing inference. Translation fields and logic are removed throughout.

### User defaults and request precedence

Registry YAML contains artifact and model capability metadata only. User `config.yaml` owns operational limits, accelerator/sampling settings, and mode-specific defaults: Base clone reference path and optional transcript, CustomVoice named voice, and VoiceDesign prompt/description. Sane defaults apply when omitted. A valid request-level clone, named voice, or design prompt overrides its corresponding configured default for that request only; daemon-wide sampling remains non-overridable.

## Risks / Trade-offs

- **[Terminal transient download failure]** A temporary network error disables that model until restart → expose an actionable error and rely on service restart as the explicit retry boundary.
- **[Global singleton reduces test isolation]** Global state can leak between tests → support directly constructed isolated registries and reserve global initialization for daemon composition.
- **[Package YAML and Rust loaders drift]** A syntactically valid manifest may use unsupported kinds or roles → validate kinds at startup and require each consumer to validate its role set before native loading.
- **[HashMap output is unordered]** Consumers cannot depend on iteration order → role lookup is the only supported access pattern.
- **[Revision not identity]** Package updates can redefine an existing model ID → content hashes prevent stale bytes, and startup view validation atomically replaces an obsolete view.
- **[Removing local overrides/offline policy is breaking]** Existing deployments may depend on them → document removal; pre-populating verified cache blobs remains sufficient for offline startup.

## Migration Plan

1. Add the strict registry schema, package manifest, validation, singleton initialization, cache/view resolution, and focused tests alongside existing acquisition code.
2. Migrate all curated constants into YAML and prove parity for provider/model IDs, kinds, roles, revisions, URLs, sizes, digests, and languages.
3. Introduce long-lived STT/TTS provider handles and move D-Bus state reads to their state accessors.
4. Migrate STT and TTS initialization to registry resolution and role validation.
5. Update user configuration and request decoding, including translation removal and TTS default precedence.
6. Remove `acquisition.rs`, mutable lifecycle controllers, obsolete configuration fields, and compatibility APIs.
7. Install the registry through Nix/package outputs and run unit, integration, provider smoke, offline-cache, and package validation tests.

Rollback is a source/package rollback because registry and configuration are startup-only. Content-addressed blobs remain compatible; obsolete assembled views can be ignored or rebuilt.
