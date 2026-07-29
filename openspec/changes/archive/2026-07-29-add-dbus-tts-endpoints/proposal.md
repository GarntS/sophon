## Why

Sophon advertises text-to-speech service capabilities but currently exports only transcription methods. Adding a provider-neutral TTS path now enables local file, descriptor, and PipeWire output while keeping the public API compatible with both the initial Kokoro implementation and the existing `qwentts-cpp` engine.

## What Changes

- Add `SpeakToFile`, `SpeakToBuffer`, and `SpeakAloud` methods to the existing session D-Bus interface.
- Add a Sophon-owned TTS provider abstraction with owned mono `f32` PCM results, capability reporting, named voices, and provider-neutral voice intents for named voices, one-shot cloning, and voice design.
- Implement the initial provider with `tts-rs` and its Kokoro engine; unsupported voice intents return a stable capability error rather than being silently ignored.
- Automatically acquire and SHA-256 verify all pinned Kokoro model artifacts in the XDG model cache.
- Return immutable server-created memfds from `SpeakToBuffer`, and exclusively create previously nonexistent paths for `SpeakToFile`.
- Play `SpeakAloud` output synchronously through PipeWire using a configured node name and volume, with serialized playback.
- Add independent TTS lifecycle, model, voice, capability, and download properties so STT remains available when TTS initialization fails.
- Add strict TTS request, cloning-audio, queue, text, reference-audio, and generated-output resource limits.

## Capabilities

### New Capabilities
- `speech-synthesis`: D-Bus synthesis methods, request options, WAV output, one-shot cloning input, resource limits, and stable errors.
- `synthesis-models`: Provider-neutral TTS engine contract, Kokoro model acquisition, named voices, capabilities, independent lifecycle, and serialized inference.
- `speech-playback`: Synchronous serialized PipeWire playback with configured device and volume behavior.

### Modified Capabilities
- `service-configuration`: Add validated startup-only TTS provider, model, output, default voice, and resource-limit configuration.

## Impact

The change affects D-Bus introspection and integration tests, daemon composition, domain and error types, model acquisition, configuration, worker scheduling, Nix packaging, and README documentation. It adds `tts-rs` with Kokoro support and PipeWire Rust/system dependencies, increases the default model download/cache footprint, and requires alignment with Sophon's pinned ONNX Runtime dependency. Existing transcription methods and STT lifecycle remain available and behaviorally independent.
