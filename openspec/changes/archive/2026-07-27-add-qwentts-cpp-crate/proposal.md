## Why

Sophon now packages qwentts.cpp as a native shared library, but Rust callers have no typed, ownership-safe way to load its paired models and synthesize speech. A dedicated crate establishes that boundary before Sophon adds its future provider-selection layer.

## What Changes

- Add a `qwentts-cpp` Rust crate that generates private raw bindings from qwentts.cpp's public `qwen.h` with rust-bindgen and links the installed shared `libqwen` library.
- Provide a safe, stateful `QwenTtsEngine` API modeled on the lifecycle and result ergonomics of `tts-rs`: create an engine, load or unload models, synthesize text, enumerate speakers, and receive owned audio results.
- Model Qwen-specific load and synthesis options, including paired talker/codec model paths, language, sampling, named speakers, voice cloning references, and voice-design instructions, without exposing raw C pointers or requiring callers to write unsafe Rust.
- Return concrete, contextual Rust errors derived from qwentts status values and diagnostics.
- Integrate the crate into the repository workspace and add Nix package builds that generate bindings and link each matching qwentts CPU/CUDA/MIGraphX package.
- Do not add a Sophon TTS service, provider abstraction, D-Bus API, model acquisition flow, or provider selection in this change.

## Capabilities

### New Capabilities
- `qwentts-rust-api`: Safe Rust bindings and an ergonomic Qwen TTS engine API over the qwentts.cpp C ABI.
- `qwentts-rust-build-integration`: Reproducible bindgen and native-library discovery/linking for the new crate and Nix package variants.

### Modified Capabilities

_None._

## Impact

- Adds a new sibling Cargo crate, private bindgen output, and Rust dependencies such as `bindgen` and audio-WAV writing support.
- Updates the Cargo workspace and Nix build configuration to build the wrapper against the appropriate qwentts shared-library variant.
- Depends on the installed qwentts.cpp public header and `libqwen.so`; no existing Sophon runtime behavior or public D-Bus contract changes.
