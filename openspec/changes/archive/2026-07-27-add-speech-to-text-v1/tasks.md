## 1. Prove ONNX Runtime Packaging

- [x] 1.1 Create a minimal Nix flake and CPU derivation that builds `transcribe-rs` 0.3.11 with ONNX support against an ONNX Runtime version compatible with `ort` 2.0.0-rc.12, with all runtime artifacts fetched by Nix and no `ort-sys` build-time download.
- [x] 1.2 Add a provider-registration smoke check proving the CPU execution provider loads from the Nix package at runtime.
- [x] 1.3 Prototype a CUDA derivation/feature and prove it builds or evaluates with CUDA plus CPU fallback. Record the unavailable ONNX Runtime 1.24.4 ROCm Linux archive as a known NixOS packaging limitation in `KNOWN_ISSUES.md`; defer `sophon-rocm` until a reproducible source build or validated compatible runtime is available.
- [x] 1.4 Define Cargo feature flags and NixOS flake package outputs so `default` is CPU and `sophon-cuda` compiles only its corresponding `transcribe-rs` ORT acceleration feature. Define the future `rocm` Cargo feature, but defer `sophon-rocm` per `KNOWN_ISSUES.md`.

## 2. Establish the Rust Service Structure

- [x] 2.1 Replace the placeholder layout with library modules separating configuration, domain types, audio ingestion, model acquisition, inference backends, worker scheduling, post-processing, D-Bus transport, and daemon startup.
- [x] 2.2 Add and pin the Rust dependencies needed for async D-Bus, YAML/Serde, structured errors and logging, WAV parsing, XDG paths, verified HTTPS downloads, archive extraction, hashing, file locking, and `transcribe-rs` ONNX inference.
- [x] 2.3 Define transport-independent transcript, transcription request/options, model lifecycle state, and error types, including stable conversion boundaries for public D-Bus errors.
- [x] 2.4 Add an ordered transcript post-processing abstraction with an identity implementation and tests proving raw and final transcript data remain distinct.

## 3. Implement Startup Configuration

- [x] 3.1 Define strict YAML configuration types for engine, model ID/path, quantization, accelerator, language/translation defaults, cache/download policy, audio limits, queue capacity, and logging verbosity.
- [x] 3.2 Implement XDG configuration and cache path discovery with the specified environment-variable precedence and user-home fallbacks.
- [x] 3.3 Implement documented no-file defaults for pinned Parakeet int8, automatic acquisition/acceleration, English, translation disabled, 32 MiB, 10 minutes, and queue depth 8.
- [x] 3.4 Implement cross-field and range validation that rejects malformed YAML, unknown fields, invalid model/engine combinations, unavailable explicit accelerators, invalid paths, and invalid resource limits.
- [x] 3.5 Add configuration tests for complete, partial, absent, malformed, unknown-field, inconsistent, and out-of-range configurations and verify configuration is read only at process startup.

## 4. Implement Model Registry and Acquisition

- [x] 4.1 Define a curated, versioned registry containing pinned Parakeet and Canary Hugging Face revisions, HTTPS file URLs, SHA-256 file manifests, quantizations, language/capability metadata, and expected layouts.
- [x] 4.2 Implement local model-path override validation and ensure an override bypasses registry lookup and network access without download fallback on failure.
- [x] 4.3 Implement XDG cache lookup and validation so a complete cached model can start offline and partial or malformed cache entries are rejected.
- [x] 4.4 Implement streamed automatic downloads with proxy-aware HTTPS, progress reporting, cross-process locking, temporary storage, per-file SHA-256 verification, layout validation, and atomic publication.
- [x] 4.5 Add deterministic acquisition tests using local fixtures/fake HTTP responses for cache hits, interrupted downloads, digest mismatch, invalid layouts, concurrent acquisition, and atomic recovery.
- [x] 4.6 Implement the model lifecycle state machine and observable snapshot updates for `Initializing`, `Downloading`, `Loading`, `Ready`, and `Failed`, including progress and actionable last-error details.

## 5. Integrate Transcription Backends

- [x] 5.1 Implement the backend factory for configured Parakeet model directories and quantization using the common `transcribe-rs::SpeechModel` interface.
- [x] 5.2 Implement the backend factory for configured Canary model directories and quantization, including multilingual and translate-to-English capability handling.
- [x] 5.3 Apply ORT accelerator selection before model construction; allow `auto` CPU fallback and convert unavailable or failed explicit CUDA/ROCm selections into initialization failure.
- [x] 5.4 Validate per-request language and translation options against active model capabilities and map them to `transcribe-rs::TranscribeOptions` without model reload.
- [x] 5.5 Add backend construction and option-validation tests using model-independent fixtures/mocks, plus opt-in smoke tests for real Parakeet and Canary models.

## 6. Implement Audio Ingestion and Scheduling

- [x] 6.1 Implement shared WAV parsing that accepts only complete mono 16 kHz signed 16-bit PCM and yields canonical `f32` samples with encoded-size and decoded-duration validation.
- [x] 6.2 Implement file ingestion requiring an absolute path to a regular file, opening it once and mapping path, access, type, format, and resource failures to domain errors.
- [x] 6.3 Implement Unix-FD ingestion for any readable seekable descriptor, seek to offset zero, preserve ownership safety, and reject non-seekable or malformed inputs.
- [x] 6.4 Implement a dedicated blocking model worker with a configurable bounded FIFO channel, one-shot responses, serialized mutable model access, and full-queue rejection.
- [x] 6.5 Implement the application transcription service that checks readiness, validates options/audio before queueing, invokes the post-processing pipeline, and returns final text.
- [x] 6.6 Add unit and concurrency tests covering valid file/FD inputs, every rejected WAV property, size/duration limits, FIFO serialization, queue saturation, readiness errors, worker inference errors, and continued daemon operation after failures.

## 7. Expose the D-Bus Service

- [x] 7.1 Implement the session-bus object at `/com/garntresearch/sophon` on interface `com.garntresearch.sophon` with PascalCase `TranscribeFile(s, a{sv}) -> s` and `TranscribeMemfd(h, a{sv}) -> s` methods.
- [x] 7.2 Implement strict D-Bus option-dictionary decoding for `language` and `translate`, including configured defaults and rejection of unknown keys, wrong types, unsupported languages, and unsupported translation.
- [x] 7.3 Expose read-only `State`, `ActiveEngine`, `ActiveModel`, `DownloadProgress`, and `LastError` properties and emit standard property-change notifications for lifecycle updates.
- [x] 7.4 Map domain failures to stable Sophon D-Bus error names for `NotReady`, `InvalidOptions`, `InvalidAudio`, `ModelUnavailable`, `ResourceLimit`, and `TranscriptionFailed`.
- [x] 7.5 Implement daemon startup so it claims the bus promptly, starts configuration/model initialization in the background, remains available in `Failed`, handles shutdown cleanly, and never initializes a GUI or audio-capture subsystem.
- [x] 7.6 Add isolated session-bus integration tests for introspection signatures, PascalCase method dispatch, Unix FD transfer, properties/signals, each public error, and concurrent request behavior without downloading large models.

## 8. Complete Nix Service Integration and Documentation

- [x] 8.1 Install `com.garntresearch.sophon.service` under `share/dbus-1/services` in every package variant with an absolute package-store daemon path and verify session-bus activation.
- [x] 8.2 Add flake apps, development shell, formatting/lint/test checks, D-Bus integration checks, configuration/cache tests, provider smoke checks, and supported Linux-system package evaluation.
- [x] 8.3 Verify the CPU closure contains no GUI, display-server, audio-capture, CUDA, or ROCm dependencies and verify each GPU closure contains only its selected vendor stack.
- [x] 8.4 Document installation and variant selection, `config.yaml` schema/defaults, model downloads/cache/path overrides, D-Bus methods/options/properties/errors, strict WAV requirements, client timeout guidance, restart semantics, and Linux-only scope.
- [x] 8.5 Run Rust formatting, static analysis, unit/integration tests, `nix flake check`, sandboxed CPU build, CUDA/ROCm build or provider validation on suitable builders, and an end-to-end transcription smoke test before declaring version 1 complete.
