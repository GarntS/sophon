## 1. Extend the qwentts Rust API

- [x] 1.1 Mark `QwenTtsEngine` `Send` with a pinned-ABI safety justification, assert that it is `Send` and not `Sync`, and test moving an unloaded engine between threads
- [x] 1.2 Expose safe loaded-engine duration-to-token conversion with unloaded and invalid-input error coverage
- [x] 1.3 Add a safe process-wide qwentts log callback API with mapped levels, copied UTF-8 messages, callback replacement/restoration, and panic containment tests
- [x] 1.4 Update qwentts-cpp API documentation for thread movement, duration conversion, and callback threading semantics

## 2. Build the shared artifact cache

- [x] 2.1 Add expected byte sizes and content-addressed artifact identities to acquisition manifests while preserving exact Kokoro and STT verification
- [x] 2.2 Implement `artifacts/<sha256>/<filename>` path resolution and independent size-plus-digest validation
- [x] 2.3 Implement per-digest locking, temporary streaming downloads, flush and verification, atomic publication, and failed-temporary cleanup
- [x] 2.4 Replace per-file completion progress with aggregate byte-level progress that credits already verified artifacts
- [x] 2.5 Add tests for shared-artifact reuse, corruption rejection, concurrent acquisition, interrupted downloads, retained completed artifacts, and monotonic byte progress
- [x] 2.6 Preserve or safely migrate existing curated Kokoro cache behavior and test startup with a pre-change valid cache

## 3. Register curated Qwen models

- [x] 3.1 Add exact immutable URLs, byte sizes, and SHA-256 digests for all five Q8_0 talkers and the shared Q8_0 codec at revision `e0f336a048a3de02b29b8ad92969217d9ecffe3e`
- [x] 3.2 Define stable model IDs and typed Base, CustomVoice, and VoiceDesign metadata, with 0.6B Base, 0.6B CustomVoice, and 1.7B VoiceDesign as mode defaults
- [x] 3.3 Resolve each Qwen model to explicit verified talker and shared codec paths and enforce provider/model agreement
- [x] 3.4 Enforce curated-only local Qwen overrides with exact artifact validation and no fallback download
- [x] 3.5 Add registry tests covering all five models, shared codec identity, defaults, provider/mode metadata, and rejection of unregistered or mismatched models

## 4. Introduce typed TTS configuration

- [x] 4.1 Refactor TTS configuration into shared operational settings plus strict Kokoro, Qwen Base, Qwen CustomVoice, and Qwen VoiceDesign variants
- [x] 4.2 Preserve absent-TTS Kokoro defaults and reject fields that are inapplicable to the selected provider/model variant
- [x] 4.3 Add CustomVoice default `vivian` and VoiceDesign default `A warm, clear, natural adult voice with moderate pitch and pace.` with model-aware validation
- [x] 4.4 Add strict daemon-wide Qwen sampling configuration with random seed, 2048 tokens, temperature 0.9, top-k 50, top-p 1.0, and repetition penalty 1.05 defaults
- [x] 4.5 Validate sampling ranges, unit-only Qwen speed, and independent byte/control-character limits for default descriptions
- [x] 4.6 Add configuration tests for every variant, partial defaults, all five model selections, deterministic seed policy, malformed values, and cross-variant fields

## 5. Implement the Qwen providers

- [x] 5.1 Add the shared Qwen engine adapter for model loading, error conversion, sampling, native duration token caps, and owned 24 kHz mono PCM results
- [x] 5.2 Implement conservative case-insensitive Qwen language normalization with automatic detection for omission and rejection for unsupported tags
- [x] 5.3 Implement `QwenTtsBaseProvider` default and one-shot clone synthesis, including optional clone transcripts and temporary native voice-reference ownership
- [x] 5.4 Implement `QwenTtsCustomVoiceProvider` speaker enumeration, configured default speaker validation, and named-voice synthesis
- [x] 5.5 Implement `QwenTtsVoiceDesignProvider` configured default-description and per-request override synthesis
- [x] 5.6 Add provider tests for capabilities, valid and invalid intents, language mapping, independent text-like limits, unit speed, sampling propagation, and failure recovery

## 6. Integrate provider selection and observability

- [x] 6.1 Add a typed provider factory that constructs Kokoro or the correct Qwen wrapper from validated registry metadata
- [x] 6.2 Add `speed_control` to provider capabilities, advertise it over D-Bus as `speed-control`, and reject unsupported speed before queueing
- [x] 6.3 Apply `max_text_bytes` independently to synthesis text, clone transcript, request description, and configured default description
- [x] 6.4 Install the qwentts log bridge during daemon startup and map native levels into corresponding `tracing` events
- [x] 6.5 Extend lifecycle and D-Bus tests for every Qwen mode, provider/model identity, available voices, capabilities, download progress, and isolated initialization failures
- [x] 6.6 Add an opt-in real-model smoke harness that verifies finite nonempty 24 kHz output for default, named, clone, and design paths without running in ordinary model-free checks

## 7. Package native backends

- [x] 7.1 Add the root qwentts-cpp dependency with defaults disabled and define mutually exclusive CPU, CUDA, and Vulkan Qwen feature selection
- [x] 7.2 Pair `sophon-cpu` with Qwen CPU, `sophon-cuda` with Qwen CUDA, and `sophon-migraphx` with Qwen Vulkan in Cargo and Nix
- [x] 7.3 Add CMake, OpenBLAS, compiler, bindgen, CUDA, and Vulkan build prerequisites to the applicable Sophon derivations and development shell
- [x] 7.4 Install `libqwen` and common/backend GGML shared libraries into each Sophon output and set relocatable binary and library RPATHs
- [x] 7.5 Add package checks for native library presence, installed-daemon loading, exact backend selection, and CPU/CUDA/MIGraphX-plus-Vulkan closure policy
- [x] 7.6 Run Rust formatting, clippy, unit/integration tests, and supported Nix flake checks and resolve all regressions

## 8. Document operation and limitations

- [x] 8.1 Document all five Q8_0 model IDs, shared codec storage, model footprints, typed configuration examples, defaults, languages, capabilities, and sampling policy in `README.md`
- [x] 8.2 Document serialized non-cancellable Qwen inference and abandoned-request head-of-line blocking as a performance blocker in `KNOWN_ISSUES.md`
- [x] 8.3 Document CPU, CUDA, and MIGraphX/Vulkan backend pairings, runtime requirements, and model-free versus heavyweight validation procedures
