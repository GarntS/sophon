## Context

Sophon already separates D-Bus synthesis, request decoding, a serialized `TtsProvider` worker, provider-native PCM output, playback, model acquisition, and TTS lifecycle reporting. Kokoro is the only implementation. The workspace also contains a safe `qwentts-cpp` crate that builds the pinned qwentts.cpp C ABI and exposes model loading, synthesis, speaker enumeration, clone-reference extraction, languages, and sampling controls.

Qwen checkpoints divide behavior by model mode rather than enabling every voice intent in one checkpoint. Base provides default output and cloning, CustomVoice provides named speakers, and VoiceDesign requires an instruction. Each checkpoint loads a talker GGUF and a codec GGUF; all selected models share the same codec. qwentts.cpp selects exactly one native backend at compile time.

## Goals / Non-Goals

**Goals:**
- Add type-safe Qwen Base, CustomVoice, and VoiceDesign providers behind the existing provider-neutral API.
- Support all five curated Q8_0 talkers from a pinned Hugging Face revision with one shared verified codec artifact.
- Preserve strict startup configuration, bounded requests, provider capability discovery, independent TTS lifecycle, and complete 24 kHz float PCM output.
- Package a matching qwentts backend in every Sophon Nix variant and route native logs through `tracing`.
- Reuse content-addressed model artifacts and expose accurate byte-level download progress.

**Non-Goals:**
- Runtime provider/model switching without restarting the daemon.
- Arbitrary local, converted, or unpinned Qwen GGUF files.
- Per-request sampling overrides, streaming D-Bus output, concurrent inference, cancellation, or automatic request abandonment when a client disconnects.
- Qwen speed alteration or output time stretching.
- SYCL-backed Qwen integration in a Sophon service package.

## Decisions

### Use three mode wrappers over one shared Qwen engine adapter

`QwenTtsBaseProvider`, `QwenTtsCustomVoiceProvider`, and `QwenTtsVoiceDesignProvider` each own a shared internal `QwenEngine` adapter. The adapter handles native model lifecycle, language conversion, daemon sampling policy, duration-to-token limiting, error conversion, and owned PCM conversion. Each wrapper handles only its valid voice intents and advertises fixed capabilities. All three report provider ID `qwentts-cpp`; the selected model ID distinguishes mode, size, and quantization.

This is preferred over one mode-switching provider because impossible states and defaults remain local to concrete types, while shared native mechanics are not duplicated.

### Mark the safe engine `Send`, but not `Sync`

`QwenTtsEngine` owns an opaque `qt_context` through `NonNull`, so Rust cannot derive `Send`. The current ABI documents synthesis as thread-safe and serializes GPU access within the context. The wrapper will add a documented `unsafe impl Send for QwenTtsEngine` so an exclusively owned provider can be loaded on a blocking thread and moved into Sophon's dedicated worker. It will not implement `Sync`; safe Rust access remains exclusive through mutable engine methods. Compile-time assertions will cover the intended `Send` and non-`Sync` contract.

This is preferred over redesigning worker startup solely to avoid moving an FFI handle whose ABI permits cross-thread use.

### Use strict typed TTS configuration variants

After decoding provider and model discriminators, configuration is converted into one of `Kokoro`, `QwenBase`, `QwenCustomVoice`, or `QwenVoiceDesign` variants containing shared operational settings plus mode-specific fields. The existing flat `tts` YAML section remains recognizable, but fields invalid for the selected variant are rejected.

Qwen Base has no mode-specific default. CustomVoice requires `default_voice` and defaults to `vivian`. VoiceDesign requires `default_voice_description` and defaults to `A warm, clear, natural adult voice with moderate pitch and pace.` Per-request `voice_description` overrides that default for one call. Descriptions are trimmed, non-empty, bounded by `max_text_bytes`, allow Unicode and normal whitespace, and reject NUL and non-whitespace control characters.

Kokoro remains the default when `tts` is omitted, preserving existing deployments.

### Configure one daemon-wide sampling policy

Qwen configuration accepts a strict `sampling` mapping with optional `seed`, `max_new_tokens`, `temperature`, `top_k`, `top_p`, and `repetition_penalty`. Requests cannot override it. Omitted fields use upstream-aligned defaults: random seed, 2048 tokens, temperature 0.9, top-k 50, top-p 1.0, and repetition penalty 1.05. Values must be finite and within conservative valid ranges established by the safe wrapper.

The effective token cap is the lesser of configured `max_new_tokens` and the native `duration_sec_to_tokens(max_generated_audio_seconds)` result. The existing post-synthesis duration check remains defense in depth.

### Advertise speed support explicitly

`TtsCapabilities` gains `speed_control`, exposed as `speed-control` in `TtsCapabilities`. Kokoro advertises it; every Qwen wrapper does not. Qwen configuration requires unit default speed, and Qwen requests with a non-unit speed fail before inference. This is preferred over silently ignoring a public request option or adding time-stretch DSP.

### Normalize only known Qwen languages

A missing language maps to native automatic detection. Case-insensitive supported base and regional tags map to Qwen's English, Chinese, Japanese, Korean, German, French, Russian, Portuguese, Spanish, and Italian language values. Recognized regional tags collapse to their supported base language. Unknown tags fail with `InvalidTtsOptions`; they never silently become English. CustomVoice model metadata remains responsible for its documented speaker dialect behavior.

Synthesis text, clone transcript, request voice description, and configured default description are independently checked against `max_text_bytes` before native work.

### Pin all five Q8_0 talkers and one shared codec

The curated registry uses immutable revision `e0f336a048a3de02b29b8ad92969217d9ecffe3e` from `Serveurperso/Qwen3-TTS-GGUF` and exact sizes and SHA-256 digests. It registers 0.6B and 1.7B Base, 0.6B and 1.7B CustomVoice, and 1.7B VoiceDesign talkers, plus `qwen-tokenizer-12hz-Q8_0.gguf`. Defaults within Qwen modes are 0.6B Base, 0.6B CustomVoice, and 1.7B VoiceDesign.

Local overrides remain curated-only: a configured model location must match the selected manifest exactly and never falls back to another artifact or automatic replacement.

### Store model files as shared content-addressed artifacts

The TTS cache stores each file beneath `artifacts/<sha256>/<filename>`. Model definitions reference talker and codec artifact records, and resolution returns explicit verified paths instead of requiring duplicated model directories or symlinks. Per-digest locks coordinate concurrent acquisition. Downloads stream into a temporary file beside the artifact, update the digest and progress per chunk, flush, verify exact size and SHA-256, and atomically rename. A failed talker download does not invalidate an already verified codec.

Progress is total verified or downloaded bytes divided by total expected bytes for all artifacts required by the selected model. Already valid artifacts count immediately. The acquisition abstraction applies the same byte-level behavior to existing manifests where expected sizes are added.

### Pair native backends with Sophon package variants

The root dependency disables qwentts-cpp default features and each package selects exactly one backend:

| Package | STT backend | Qwen backend |
|---|---|---|
| `sophon-cpu` | ONNX Runtime CPU | GGML CPU/OpenBLAS |
| `sophon-cuda` | ONNX Runtime CUDA | GGML CUDA |
| `sophon-migraphx` | ONNX Runtime MIGraphX | GGML Vulkan |

Cargo feature combinations that select zero or multiple Qwen backends fail early. Nix supplies CMake, bindgen/libclang, OpenBLAS, compiler runtimes, and backend-specific CUDA or Vulkan prerequisites. Sophon outputs copy `libqwen` and required GGML shared libraries into `$out/lib`, set `$ORIGIN`-based library RPATHs, and verify closures do not acquire unrelated accelerators.

The standalone qwentts package outputs remain available.

### Bridge native logging into `tracing`

The safe wrapper exposes process-wide log callback installation using a panic-contained, thread-safe Rust callback and maps native levels to a Rust enum. Sophon installs one callback during startup and maps debug, info, warning, and error events into the corresponding `tracing` levels. Native messages are copied before returning from the C callback. The bridge must remain reentrant because qwentts.cpp may log from internal worker or caller threads.

### Retain serialized non-cancellable inference

The existing bounded FIFO worker remains unchanged semantically. A running request continues after caller disconnection and blocks later inference until completion; queued requests cannot be cancelled. `KNOWN_ISSUES.md` will identify this as a performance blocker, note that queue capacity bounds pending work but not execution time, and record native cooperative cancellation as a future integration path.

## Risks / Trade-offs

- **Large downloads and memory use** → Pin exact Q8_0 sizes, provide byte progress, share the codec, retain configurable automatic download, and document footprints.
- **Unsafe `Send` assertion becomes invalid upstream** → Tie the safety comment to the pinned ABI guarantee, avoid `Sync`, and review the assertion whenever qwentts.cpp is updated.
- **Content-addressed cache migration complicates existing caches** → Recognize and verify existing Kokoro entries during migration or reacquire them atomically; never delete a valid old cache before the replacement is published.
- **Global native logging callback affects every engine** → Install it once during daemon startup, use a reentrant callback, and avoid callback-owned engine state.
- **GPU package closure growth or missing runtime libraries** → Add backend-specific RPATH, library-presence, evaluation, and closure-policy checks.
- **Fixed sampling policy limits client control** → Keep behavior reproducible when a seed is configured and defer per-request controls until a concrete API requirement exists.
- **Serialized CPU synthesis may cause long head-of-line blocking** → Keep queues bounded and document the limitation and future cancellation route.

## Migration Plan

1. Extend qwentts-cpp's safe API and backend-independent tests.
2. Introduce artifact records and migrate acquisition to verified byte-progress storage while preserving existing Kokoro behavior.
3. Add typed configuration and registry entries without changing the default Kokoro selection.
4. Add Qwen wrappers and provider factory selection, then update D-Bus capabilities and validation.
5. Integrate each native backend into Nix packages and checks.
6. Update user configuration, model footprint, capability, performance, and packaging documentation.

Rollback selects the existing Kokoro provider/model configuration or reverts to the prior package. Old content-addressed artifacts are inert when unreferenced and can be removed manually; no user-generated data format is changed.
