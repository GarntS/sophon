## 1. Workspace and native binding setup

- [x] 1.1 Add `qwentts-cpp` as a workspace crate with its package metadata and runtime/build dependencies.
- [x] 1.2 Implement the crate build script to validate `QWENTTS_INCLUDE_DIR` and `QWENTTS_LIB_DIR`, generate private allowlisted bindings from `qwen.h`, emit dynamic `qwen` link directives, and declare native-header/environment rebuild triggers.
- [x] 1.3 Add the private raw-bindings module and verify that generated C ABI types and functions are not exported by the crate's public API.
- [x] 1.4 Add build-script coverage or validation for missing and supplied native-discovery configuration, including actionable missing-configuration diagnostics.

## 2. Safe engine lifecycle and audio result

- [x] 2.1 Define the crate's concrete error type, C-string/path conversion validation, and native status/diagnostic translation.
- [x] 2.2 Implement `QwenTtsEngine` construction, paired-model loading with defaultable model options, unload/reload behavior, and `Drop` resource release.
- [x] 2.3 Implement safe synthesis with defaultable request options and an unloaded-engine error path.
- [x] 2.4 Implement an owned `SynthesisResult` with f32 samples, sample rate, duration calculation, and floating-point WAV-file writing.
- [x] 2.5 Add lifecycle and result tests that exercise unloaded behavior, native-free ownership boundaries where testable without a model, and WAV output.

## 3. Qwen-specific safe options

- [x] 3.1 Define safe language, sampling, and `Voice` domain types that map default, named-speaker, clone, clone-with-transcript, and voice-design requests to the native ABI.
- [x] 3.2 Implement safe `VoiceReference` extraction from mono 24 kHz f32 PCM and automatic native reference-buffer release.
- [x] 3.3 Implement safe speaker enumeration that returns Rust-owned names and handles models without named speakers.
- [x] 3.4 Add tests for option validation, C-string rejection, voice-reference cleanup, and native failure diagnostic preservation using feasible fixtures or ABI-level tests.

## 4. Nix integration and verification

- [x] 4.1 Add dedicated Nix package outputs for the crate that pair it with the CPU, CUDA, and MIGraphX qwentts packages without making Sophon depend on the crate.
- [x] 4.2 Provide declared bindgen/libclang prerequisites and the qwentts include/library environment to each Nix crate build.
- [x] 4.3 Ensure each crate package runtime closure retains its selected qwentts shared library and backend dependencies.
- [x] 4.4 Add Nix checks for the crate package variants and run formatting, crate tests, and applicable Nix builds.

## 5. Documentation and change validation

- [x] 5.1 Document native build configuration, the tts-rs-shaped engine lifecycle, paired model loading, voice modes, and the absence of streaming/provider abstraction in the initial API.
- [x] 5.2 Validate the OpenSpec change and resolve any validation findings before implementation begins.
