## Context

qwentts.cpp exposes a narrow C99 ABI in `src/qwen.h` and the repository's Nix flake already produces variant-specific shared `libqwen.so` packages with the header installed. The ABI owns opaque model contexts, malloc-backed audio and voice-reference buffers, and uses status codes plus a thread-local diagnostic string for failures. Its inputs contain raw pointers, C strings, callbacks, and combinations of fields whose validity depends on the loaded Base, CustomVoice, or VoiceDesign model.

The new crate must make that ABI usable from Rust while retaining the familiar lifecycle and owned audio result of `tts-rs`. Sophon is not yet a TTS service and must not acquire a provider-neutral abstraction in this change.

## Goals / Non-Goals

**Goals:**
- Add a standalone `qwentts-cpp` Cargo crate with private generated FFI bindings.
- Present a `tts-rs`-style stateful engine: construct, load paired models, synthesize, unload, and automatically release native state.
- Return Rust-owned PCM results with WAV-writing and duration helpers.
- Represent Qwen-specific voice modes, sampling controls, speaker enumeration, and reusable voice references without exposing pointer ownership or unsafe calls to consumers.
- Make native discovery/linking explicit and reproducible for the existing CPU, CUDA, and MIGraphX Nix packages.

**Non-Goals:**
- Add a Sophon TTS D-Bus API, configuration, model downloading, or provider-selection abstraction.
- Build qwentts.cpp from source in Cargo or support static linking in the initial crate.
- Expose raw generated bindings, the global native logging callback, or callback-based streaming in the initial safe API.
- Promise Rust-level concurrent sharing of an engine or surface qwentts internal batching initially.

## Decisions

### Make generated bindings private and wrap the public C ABI

`build.rs` will run rust-bindgen over the installed `qwen.h`, emit bindings into `OUT_DIR`, and a private module will include them. Public modules will contain only Rust types and methods.

This preserves synchronization with the upstream C ABI while preventing raw structs, callbacks, and pointers from becoming a semver commitment. A safe wrapper still needs a small, documented internal unsafe boundary for FFI calls and C-owned buffers; the public crate API will require no unsafe Rust.

**Alternative considered:** Commit generated bindings to source. This weakens header/build consistency and duplicates bindgen output in version control.

### Link an externally built shared qwentts library

The build script will obtain a header directory and native library directory from explicit `QWENTTS_INCLUDE_DIR` and `QWENTTS_LIB_DIR` build environment variables, generate bindings from the header, and emit dynamic-link directives for `qwen`. It will fail clearly when either location is unavailable.

The initial crate links only the shared artifact already built by Nix, which encapsulates qwentts.cpp's C++ and ggml dependency graph. Dedicated Nix builds for this crate will pass the matching CPU, CUDA, or MIGraphX package paths and preserve each selected package's runtime library closure. Sophon does not depend on the crate until its future TTS integration.

**Alternative considered:** Invoke CMake and compile the vendored C++ tree in every Cargo build. That duplicates a large backend-sensitive native build, conflicts with the existing Nix packaging model, and makes accelerator selection opaque.

### Use a tts-rs-shaped, mutable engine lifecycle

The public API will center `QwenTtsEngine`, with `new`/`Default`, `load_model`, `unload_model`, `synthesize`, and speaker-listing methods. The engine begins unloaded; synthesis before a successful load returns a typed error. It owns the native context after loading, replaces or frees it during unload/reload, and frees it in `Drop`.

`load_model` takes separate talker and codec paths because qwentts requires both; model load options carry native settings with safe defaults. `synthesize` takes text and an optional request-options value, mirroring `tts-rs` request defaults. The initial engine is intentionally used through `&mut self`, aligning with `tts-rs` and Sophon's existing serialized model-worker design.

**Alternative considered:** A builder that returns only a permanently loaded context. It makes unload/reload and direct familiarity with `tts-rs` worse without helping Sophon's future provider layer.

### Return a Rust-owned synthesis result

`SynthesisResult` will expose owned `Vec<f32>` samples and sample rate, with `duration_secs` and `write_wav`. The wrapper will copy successful C audio into the vector and then release the C buffer with its paired native free function.

Copying is intentional: converting a C `malloc` allocation into `Vec` would couple deallocation to Rust's allocator. The one output-buffer copy is an acceptable cost for a simple, portable owned result.

### Encode Qwen voice intent as Rust types

Request options will contain language and sampling settings plus a `Voice` choice that distinguishes default output, named CustomVoice speakers, Base-model clone references (with optional transcript), and VoiceDesign instructions. `VoiceReference` will own extracted native reference data and free it in `Drop`; it is created by the loaded engine from 24 kHz mono f32 reference audio.

The native library remains the authority on whether an intent matches the loaded checkpoint. Its failure status and diagnostic become a contextual crate error. This avoids reimplementing upstream model metadata validation while avoiding the invalid raw-field combinations in `qt_tts_params`.

**Alternative considered:** Expose every C parameter field directly. That leaks unsafe lifetimes and permits nonsensical combinations such as named speakers plus a clone reference.

### Keep the future Sophon abstraction above this crate

A later Sophon-owned provider interface can translate provider-neutral requests into `QwenTtsEngine` calls and can serialize work as the current STT `ModelWorker` does. It will own policy decisions about shared options and capabilities. This crate remains Qwen-specific and is not made to implement a premature generic trait.

## Risks / Trade-offs

- [The upstream C ABI changes] → Bind from the installed header on every build, keep raw bindings private, and test the supported safe surface against the current ABI.
- [Bindgen requires libclang and correct target flags] → Add it to Nix native build inputs, pass the Cargo target to clang where required, and report missing native-discovery variables with actionable diagnostics.
- [A native library or its ggml dependencies are absent at runtime] → Link only the Nix-produced shared package and ensure Sophon's eventual package runtime path includes the selected qwentts package closure.
- [Native callbacks, batching, and logging have subtle cross-thread contracts] → Exclude them from the first safe API rather than expose an unsound or misleading abstraction.
- [Voice mode validity depends on checkpoint metadata] → Use a typed intent API for invalid combinations under Rust control, then preserve native mode-validation errors for model-dependent mismatches.
- [Copying synthesized audio uses additional peak memory] → Return a conventional owned Rust result; streaming can be designed later for applications where that cost matters.

## Migration Plan

1. Add the crate as a workspace member without changing Sophon's existing STT dependency graph or D-Bus behavior.
2. Build and test the crate's dedicated Nix outputs against each qwentts package variant.
3. Keep existing Sophon and qwentts package artifacts and behavior unchanged.
4. Roll back by removing the new workspace member and its Nix wiring; no persisted configuration, external interface, or model data requires migration.
