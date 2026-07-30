## Why

The audio backend currently couples playback directly to PipeWire, buffers every synthesis before playback, and rejects otherwise usable STT WAV inputs solely because their channel layout, encoding, or sample rate differs from the model contract. Reworking the backend around CPAL, Hound, and Rubato will reduce custom low-level audio code while enabling low-latency streamed speech and model-correct STT input conversion.

## What Changes

- Replace Sophon's direct PipeWire playback implementation with CPAL's PipeWire host and callback-driven output streams.
- Replace `tts.pipewire_node` with a strict optional `tts.output_device` CPAL device ID; omission selects CPAL's current default PipeWire output device.
- Add provider-neutral streaming playback capable of consuming audio chunks while synthesis is still running.
- Expose qwentts.cpp's native audio-chunk callback through the safe Rust wrapper and use it for `SpeakAloud`.
- Preserve buffered synthesis for `SpeakToFile`, `SpeakToBuffer`, and providers without streaming support.
- Expand STT WAV ingestion to normalize Hound-supported integer and floating-point PCM, downmix multiple channels to mono, and use Rubato when the source rate differs from the active model's advertised input rate.
- Keep TTS output at the provider-native sample rate; Sophon will not resample output and will return `PlaybackFailed` when CPAL cannot open the requested format.
- Remove the direct `pipewire` dependency and obsolete PipeWire-specific playback code and tests; CPAL may continue to use PipeWire transitively through its backend feature.
- **BREAKING**: `tts.pipewire_node` is removed in favor of the canonical CPAL `tts.output_device` identifier format.
- **BREAKING**: STT accepts a broader WAV contract, and duration validation is based on normalized model-input audio rather than requiring canonical mono 16 kHz signed 16-bit input.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `speech-playback`: Replace direct PipeWire playback with CPAL device selection, provider-native callback playback, and streaming drain/error behavior.
- `speech-transcription`: Broaden WAV decoding, define channel downmixing, and resample STT input to the active model's advertised rate.
- `service-configuration`: Replace the PipeWire node setting with a CPAL device ID and retain strict startup validation.
- `speech-synthesis`: Define streamed `SpeakAloud` behavior, partial-audio failure semantics, and bounded streamed output.
- `qwentts-rust-api`: Add a safe streaming synthesis callback API over the native qwentts.cpp ABI.
- `qwen-tts-providers`: Advertise and use Qwen streaming synthesis for aloud playback while preserving buffered output endpoints.

## Impact

Affected areas include `src/audio.rs`, STT service composition, TTS provider and worker contracts, `src/tts/playback.rs`, startup configuration, qwentts.cpp Rust bindings, D-Bus behavior documentation, Cargo/Nix dependencies, unit tests, and the PipeWire playback smoke harness. The public D-Bus method signatures remain unchanged, but playback configuration and some `SpeakAloud` failure timing semantics change.
