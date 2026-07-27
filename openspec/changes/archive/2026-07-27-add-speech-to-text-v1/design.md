## Context

Sophon is an empty Rust 2024 binary. Version 1 must become a Linux-only, headless, per-user D-Bus service that transcribes short WAV recordings with local `transcribe-rs` ONNX models. The service must support Parakeet and Canary, automatic verified model acquisition, CPU/CUDA/ROCm Nix packages, startup YAML configuration, and future transcript post-processing without exposing those implementation details through D-Bus.

`transcribe-rs` local models are synchronous, implement `SpeechModel: Send`, require mutable access for inference, and accept canonical 16 kHz mono samples. ORT execution providers are compile-time features selected globally before sessions load. D-Bus activation and first-use model downloads create a readiness problem because downloading cannot safely block bus-name acquisition or a method call.

## Goals / Non-Goals

**Goals:**

- Provide stable `TranscribeFile` and `TranscribeMemfd` request/response methods on the user session bus.
- Keep the daemon responsive while downloading, loading, and running blocking inference.
- Support one configured Parakeet or Canary model with per-request language/translation options.
- Make model state observable and failures actionable through D-Bus.
- Produce reproducible CPU, CUDA, and ROCm Nix variants without network access during a Nix build.
- Separate transport, application workflow, model backend, model acquisition, and post-processing boundaries.

**Non-Goals:**

- Text-to-speech, audio capture, streaming transcription, a GUI, system-bus operation, or non-Linux support.
- Concurrent inference, multiple simultaneously loaded models, per-request engine selection, or hot configuration reload.
- General audio decoding/resampling or a long-running job API.
- LLM post-processing implementation; version 1 contains only an identity pipeline boundary.

## Decisions

### Use one conventional D-Bus object and interface

The daemon owns `com.garntresearch.sophon` on the session bus, exports `/com/garntresearch/sophon`, and uses interface `com.garntresearch.sophon`. Methods are PascalCase:

```text
TranscribeFile(s path, a{sv} options) -> s text
TranscribeMemfd(h fd, a{sv} options)  -> s text
```

The options dictionary initially recognizes `language` (string) and `translate` (boolean). Omitted options inherit configured defaults. Unknown keys, wrong value types, unsupported languages, and unsupported translation requests return `InvalidOptions`; explicit rejection avoids silent behavioral differences across server versions. Translation means translation to English, matching `transcribe-rs::TranscribeOptions`.

The interface also exposes read-only `State`, `ActiveEngine`, `ActiveModel`, `DownloadProgress`, and `LastError` properties and emits standard property-change notifications. This preserves exactly two transcription methods while allowing clients to react to readiness.

Alternatives considered: snake-case members were rejected in favor of conventional D-Bus naming; a typed options struct was rejected because every added field changes the D-Bus signature; a separate status method was rejected because properties provide standard introspection and notifications.

### Separate D-Bus, application, and inference boundaries

The D-Bus adapter converts path/FD arguments and variants into domain requests and maps domain failures to stable D-Bus errors. A transport-independent transcription service validates and schedules requests. A backend factory constructs a `Box<dyn SpeechModel + Send>` for the configured Parakeet or Canary model. A transcript domain value retains raw text, final text, segments, and engine metadata even though version 1 returns only final text.

An ordered post-processing pipeline transforms a transcript after inference. Version 1 installs an identity processor only. This boundary permits future asynchronous LLM processing without placing LLM policy in the model or D-Bus modules. TTS will use a sibling application service and separate D-Bus methods rather than extending STT backend traits.

### Own the model on one dedicated worker

A dedicated blocking worker owns the mutable model. D-Bus tasks submit validated canonical samples through a bounded FIFO channel and await one-shot responses. The default queue capacity is 8 and is configurable. A full queue returns `ResourceLimit` rather than growing memory without bound.

This serializes inference, avoids blocking the async D-Bus executor, and makes RAM/VRAM use predictable. Multi-worker model pools were rejected for version 1 because they duplicate large model state and complicate GPU scheduling. D-Bus cancellation is not relied upon: once accepted by the worker, an inference may complete even if its caller times out.

### Accept canonical WAV data only

Both methods accept complete RIFF/WAVE data containing mono, 16 kHz, signed 16-bit PCM. File paths must be absolute and identify regular files. The Unix FD method accepts any readable, seekable descriptor, seeks to offset zero, and does not require the object to literally be a Linux memfd. The service opens/owns its received descriptor and does not mutate client data.

Input is rejected before queueing when its encoded size exceeds the configured default of 32 MiB or decoded duration exceeds the configured default of 10 minutes. WAV validation and conversion produce canonical `f32` samples for the backend. Supporting arbitrary codecs or raw PCM was rejected to avoid ambiguity and headless multimedia dependencies.

### Load one configured backend at startup

Configuration selects one engine (`parakeet` or `canary`), model identifier or path, quantization, accelerator, language default, translation default, input limits, and queue capacity. The factory contains explicit model-specific construction while the application service consumes the common `SpeechModel` contract.

Accelerator selection is set before model construction. `auto` uses the best compiled provider and may fall back to CPU. An explicit provider absent from the installed binary is a configuration failure rather than the silent CPU fallback offered by the underlying library. Engine, model, quantization, and accelerator changes require daemon restart; request language and translation options do not reload the model.

### Acquire curated models asynchronously and verifiably

A built-in registry maps versioned model IDs to engine, archive URL, SHA-256 digest, expected extracted layout, and supported quantization. A configured local path bypasses downloading. Otherwise, the daemon claims its D-Bus name promptly and acquires the selected model in the background under `$XDG_CACHE_HOME/sophon/models`, falling back to `~/.cache/sophon/models`.

Downloads use HTTPS, honor conventional proxy settings, use a cross-process lock, and use temporary storage. The curated Hugging Face sources are pinned revisions containing individual ONNX files rather than a single release archive: the registry records a SHA-256 manifest for every required file, verifies each file before publication, validates the expected layout, and atomically renames the completed directory. Partial or invalid artifacts are never treated as ready. Cached valid artifacts permit offline startup. There is no automatic update of an already pinned model ID; changing IDs is explicit configuration/registry evolution.

The state machine is `Initializing -> Downloading -> Loading -> Ready`, with any state able to enter `Failed`. Calls made before `Ready` return retryable `NotReady`; calls in `Failed` return `ModelUnavailable`. Progress and failure details update D-Bus properties.

Alternatives considered: blocking the activation request risks bus/client timeout, and requiring prefetch contradicts automatic acquisition.

### Use XDG-aware startup-only YAML configuration

The daemon reads `$XDG_CONFIG_HOME/sophon/config.yaml`, falling back to `~/.config/sophon/config.yaml`. If absent, documented defaults select a pinned Parakeet int8 model, automatic acquisition, `auto` acceleration, 32 MiB/10-minute input limits, and queue capacity 8. A present invalid file moves the service to `Failed` with a configuration error rather than silently applying defaults.

Configuration is immutable for the process lifetime. Users restart the activated service after edits. Hot reload was rejected because accelerator globals and model replacement require request draining and failure-safe state transitions.

### NixOS packaging scope and ROCm limitation

Version 1 development and packaging validation target NixOS only. The Rust daemon remains Linux-oriented, but non-NixOS packaging is out of scope until this restriction is lifted.

ONNX Runtime 1.24.4, required by `ort` 2.0.0-rc.12, publishes Linux CPU and CUDA release archives but no Linux ROCm archive. `sophon-rocm` is a known limitation: it is deferred until a reproducible NixOS source build or a validated compatible runtime is available. CPU and CUDA are the supported package variants for now. The tracked user-facing record is `KNOWN_ISSUES.md`.

### Package provider-specific ONNX Runtime closures with Nix

The flake provides a CPU default plus explicit CUDA and ROCm packages, an app, development shell, and checks. Each package includes the same D-Bus activation file and only its required execution provider closure; users install exactly one variant.

Nix must supply a version-compatible ONNX Runtime without allowing `ort-sys` to download during a sandboxed Cargo build. The packaging work will pin/fetch all runtime artifacts through Nix, configure `ort` to link or dynamically load that runtime, and validate CPU, CUDA, and ROCm provider registration. Because `transcribe-rs 0.3.11` pins `ort 2.0.0-rc.12` targeting ONNX Runtime 1.24 while current Nixpkgs may differ, implementation begins with a packaging compatibility spike and pins a compatible runtime rather than relying on an unverified system version.

Alternatives considered: one broad binary produces excessive closures and runtime dependencies; build-script downloads are non-reproducible in Nix; CPU-only packaging fails the acceleration requirement.

## Risks / Trade-offs

- **[ORT/Nix incompatibility]** The pinned Rust bindings and available ONNX Runtime packages may not align, especially for ROCm. → CPU and CUDA use the pinned 1.24.4 Nix-fetched artifacts. ROCm is recorded as a known NixOS packaging limitation until a reproducible source build or validated compatible runtime is available.
- **[Large GPU closures and builds]** CUDA and ROCm variants are expensive to build and distribute. → Keep them separate from the CPU default and test provider registration independently from full model inference where hardware is unavailable.
- **[First-use latency]** Models are large and cannot be ready during immediate activation. → Claim the bus, publish download/loading progress, and return retryable `NotReady`.
- **[Long D-Bus calls]** Even short recordings may exceed client default timeouts on slower hardware. → Document extended client timeouts, enforce the ten-minute default limit, and defer job APIs.
- **[Queued memory]** Decoded audio buffers can consume significant memory. → Bound both each input and queue depth; reject excess work before inference.
- **[Path and descriptor edge cases]** Paths can change and descriptors can have unexpected types. → Open paths once after validation, require regular files for path input, require seekable FDs, and parse content rather than trusting extensions.
- **[Model registry supply chain]** Remote artifacts can disappear or be replaced. → Pin immutable URLs and SHA-256 hashes, validate layouts, and support user-managed paths/offline caches.
- **[Options dictionary weak typing]** D-Bus introspection cannot enumerate dictionary keys. → Publish the accepted schema, validate strictly, and expose stable typed errors.

## Migration Plan

There is no deployed service or persistent format to migrate. Introduce the daemon, D-Bus activation metadata, default configuration behavior, and model cache as new facilities. Installation of one flake package makes activation available on the user bus. Rollback consists of stopping the user service, removing the installed package/activation file, and optionally deleting the XDG model cache; no user source data is modified.
