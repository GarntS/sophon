## Why

Sophon currently exposes a provider-neutral synthesis API but only implements Kokoro, leaving its cloning and voice-design request contracts unavailable. The existing `qwentts-cpp` crate and pinned qwentts.cpp source make it possible to add local Qwen3-TTS providers with named voices, one-shot cloning, and voice design across the packaged CPU and GPU variants.

## What Changes

- Add Base, CustomVoice, and VoiceDesign Qwen TTS provider implementations over a shared Qwen engine adapter.
- Register all five curated Q8_0 talker models from the pinned `Serveurperso/Qwen3-TTS-GGUF` revision and one shared Q8_0 codec artifact.
- Introduce typed TTS configuration variants, including mode-specific defaults and a daemon-wide Qwen sampling policy.
- Use a configured default voice description for VoiceDesign while permitting per-request `voice_description` overrides.
- Advertise provider speed support and reject non-unit speed for Qwen rather than ignoring it.
- Store curated TTS files in a content-addressed shared artifact cache and report byte-level acquisition progress.
- Integrate qwentts native libraries into Sophon CPU, CUDA, and MIGraphX Nix packages, pairing MIGraphX STT with Vulkan Qwen TTS.
- Route native qwentts.cpp logs through Rust `tracing`.
- Document serialized, non-cancellable Qwen inference as a performance limitation.

## Capabilities

### New Capabilities
- `qwen-tts-providers`: Qwen Base, CustomVoice, and VoiceDesign behavior, curated models, language mapping, sampling, and native integration.
- `shared-model-artifacts`: Content-addressed artifact reuse, atomic verified acquisition, and byte-level progress.

### Modified Capabilities
- `speech-synthesis`: Advertise speed support and bound all text-like Qwen request inputs.
- `synthesis-models`: Select multiple curated provider/model combinations and resolve models composed from shared artifacts.
- `service-configuration`: Decode strict typed TTS variants with Qwen mode defaults and sampling policy.
- `nix-service-packaging`: Include the matching qwentts native backend and runtime libraries in each Sophon package.
- `qwentts-rust-api`: Permit moving an exclusively owned engine between threads, expose duration-to-token conversion, and bridge native logging.

## Impact

The change affects the TTS provider and worker modules, domain capabilities, request validation, startup configuration, model acquisition and lifecycle reporting, daemon provider selection, the `qwentts-cpp` safe wrapper, documentation, Cargo features, lock data, and Nix package/closure checks. Runtime closures gain qwentts.cpp, GGML, OpenBLAS, and the package-selected CUDA or Vulkan dependencies; curated Qwen downloads range from roughly 1.2 GiB to 2.2 GiB per model pair before shared-codec reuse.
