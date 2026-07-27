## Why

Sophon is currently an empty Rust executable and cannot provide local speech services to desktop or headless clients. Version 1 establishes a headless, per-user D-Bus speech-to-text service with local ONNX inference, hardware-accelerated package variants, and an architecture that can later add text-to-speech and LLM post-processing without coupling those concerns to the transport layer.

## What Changes

- Add a session-bus-activated D-Bus service named `com.garntresearch.sophon` at `/com/garntresearch/sophon`.
- Add `TranscribeFile` and `TranscribeMemfd` request/response methods that accept strict 16 kHz mono 16-bit PCM WAV audio, per-request options, and return transcribed text.
- Add D-Bus readiness properties and typed errors for model acquisition, loading, validation, resource limits, and inference failures.
- Integrate `transcribe-rs` ONNX inference with one configured active model and initial support for Parakeet and Canary.
- Add verified automatic model downloads into an XDG cache, with a user-configured local model path override.
- Add startup-only YAML configuration under the user's XDG configuration directory.
- Add bounded, serialized transcription scheduling suitable for short recordings.
- Structure transcription, model management, transport, and transcript post-processing behind separate boundaries so future LLM processors and text-to-speech APIs can be added independently.
- Add a Nix flake with separate CPU, CUDA, and ROCm package outputs, D-Bus activation metadata, development environment, and checks.
- Target Linux for version 1; no GUI or audio-capture interface is introduced.

## Capabilities

### New Capabilities

- `speech-transcription`: D-Bus file and Unix-FD transcription methods, request options, audio validation, scheduling, results, and public errors.
- `transcription-models`: Parakeet and Canary backend selection, model acquisition, model readiness state, and hardware accelerator behavior.
- `service-configuration`: XDG-aware startup configuration, defaults, validation, and restart semantics.
- `nix-service-packaging`: Reproducible CPU/CUDA/ROCm flake outputs and per-user D-Bus service activation.

### Modified Capabilities

None.

## Impact

- Replaces the placeholder executable with a long-running asynchronous D-Bus daemon and supporting library modules.
- Adds Rust dependencies for D-Bus, async execution, YAML/Serde configuration, errors/logging, downloads, hashing/archive handling, XDG paths, and `transcribe-rs` with ONNX support.
- Adds a stable public D-Bus contract at `com.garntresearch.sophon`.
- Adds runtime storage under the user's XDG cache and configuration directories and outbound HTTPS access when an uncached configured model must be downloaded.
- Adds Nix build and runtime integration with ONNX Runtime plus optional CUDA or ROCm stacks; ONNX Runtime version/linkage compatibility is a required packaging validation area.
