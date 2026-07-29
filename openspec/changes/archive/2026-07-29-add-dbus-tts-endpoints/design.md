## Context

Sophon currently composes one independently acquired STT model, a bounded synchronous model worker, and a zbus session-bus object. Its public audio paths ingest complete WAV files or transferred descriptors. TTS is absent, while the workspace now contains `qwentts-cpp`, whose safe API already returns owned mono 24 kHz `f32` PCM and supports named voices, voice cloning, and voice design. The required initial engine is `tts-rs`'s Kokoro provider, which also returns mono 24 kHz `f32` PCM and supports named voices and speed, but not cloning or voice design.

This change crosses transport, model acquisition, scheduling, configuration, audio output, PipeWire, and Nix packaging. TTS initialization must not make existing STT unavailable. Configuration remains startup-only.

## Goals / Non-Goals

**Goals:**
- Export three synchronous TTS operations for exclusive file output, immutable server-created memfd output, and serialized PipeWire playback.
- Define a Sophon-owned provider contract that can adapt both Kokoro and `qwentts-cpp` without exposing either API through D-Bus or output sinks.
- Support strict named-voice, language, speed, one-shot clone, and voice-design request intents with capability validation.
- Automatically acquire a pinned Kokoro int8 model and voice archive with SHA-256 verification and atomic cache publication.
- Keep TTS lifecycle, failures, queueing, and observable state independent from STT.
- Bound text, reference audio, generated audio, and queued work.

**Non-Goals:**
- Integrate `qwentts-cpp` as an active provider in this change.
- Resample or remix cloning reference audio; the initial clone contract is mono 24 kHz float WAV.
- Stream partial synthesis or playback to callers.
- Support cancellation after a request has entered the worker queue.
- Reload configuration or switch providers/models while the daemon is running.

## Decisions

### Put a provider-neutral owned-audio contract above engine crates

A Sophon provider interface will expose provider identity, model identity, capabilities, owned voice names, and a mutable synchronous synthesis operation. Requests use owned provider-neutral values: text, optional language and speed, and exactly one voice intent (`Default`, `Named`, `Clone` with decoded reference PCM and optional transcript, or `Design`). Results contain owned mono `Vec<f32>` samples and a sample rate.

The first adapter wraps `tts_rs::engines::kokoro::KokoroEngine`. It maps named voice and speed, validates known options, and reports cloning and voice design as unsupported. A future qwentts adapter can extract a voice reference inside a one-shot clone operation and pass the corresponding Qwen voice intent. File descriptors, WAV encoding, output paths, and PipeWire do not enter the provider API.

**Alternatives considered:** Re-export the `tts-rs` trait, which cannot represent cloning uniformly and couples Sophon to boxed engine errors; or make output destinations provider methods, which duplicates output and playback policy in every provider.

### Serialize mutable synthesis through a bounded worker

A dedicated TTS worker owns the provider on one blocking thread and accepts work through a bounded FIFO channel. All three D-Bus methods submit the same request shape. A full queue returns `ResourceLimit`; provider failure does not stop later work. Playback uses a separate serialized stage so accepted `SpeakAloud` calls never overlap, while file and memfd output remain independent after synthesis.

**Alternatives considered:** Running inference on Tokio would block the async executor. A mutex around the engine would permit unbounded waiters. Allowing concurrent playback would make intelligibility and volume behavior unpredictable.

### Use strict D-Bus request options and typed method results

The existing interface gains:

- `SpeakToFile(s text, s path, a{sv} options) -> t size_bytes`
- `SpeakToBuffer(s text, a{sv} options) -> (h fd, t size_bytes)`
- `SpeakAloud(s text, a{sv} options) -> ()`

Recognized options are `voice` (`s`), `language` (`s`), `speed` (`d`), `clone_audio` (`h`), `clone_transcript` (`s`), and `voice_description` (`s`). `voice`, `clone_audio`, and `voice_description` are mutually exclusive. `clone_transcript` requires `clone_audio`. Omitted voice and speed values use configured defaults. Unknown keys, wrong types, non-finite/out-of-range speeds, contradictory intents, unavailable voice names, and incompatible language/voice combinations are rejected before inference.

A descriptor nested in the variant dictionary is transferred and owned for request decoding just like an explicit Unix-FD argument. Clone input is read from offset zero and must contain a complete mono 24 kHz 32-bit IEEE-float WAV within configured byte and duration limits.

**Alternatives considered:** Separate cloning methods would violate the requested three-method surface. Explicit optional clone arguments make every signature bulky. Byte arrays create unnecessary D-Bus copies.

### Encode complete float WAVs outside providers

Both current engine APIs produce mono 24 kHz `f32` PCM, so file and memfd methods encode complete RIFF/WAVE with one channel, provider sample rate, and 32-bit IEEE-float samples. The output layer still checks result channels, sample rate, sample count, finite duration, and configured generated-output bounds rather than trusting a provider.

`SpeakToFile` first rejects a destination that already exists, synthesizes, then creates the absolute destination with exclusive-create semantics. A concurrent creator therefore wins safely. Any partial file is removed if WAV writing or finalization fails. It never truncates or replaces an existing filesystem object.

**Alternatives considered:** Signed 16-bit output loses provider precision. Letting providers write paths would reintroduce check/open races and make memfd output engine-specific.

### Return sealed server-created memfds

`SpeakToBuffer` creates an anonymous memfd with sealing enabled, writes and finalizes the complete WAV, rewinds it to byte zero, and applies write, grow, shrink, and further-seal seals before returning the Unix descriptor and byte length. zbus/the D-Bus transport owns the server descriptor until the reply is sent; the server then drops its reference. The client owns the transferred reference, and kernel storage is reclaimed after the final descriptor or mapping is released.

**Alternatives considered:** A caller-provided writable descriptor complicates truncation and partial-failure semantics. Returning `ay` duplicates potentially large audio in D-Bus messages.

### Use direct synchronous PipeWire playback policy

`SpeakAloud` connects through PipeWire, selects the configured stable `node.name`, submits provider-native float PCM at its declared sample rate, applies configured linear volume in `[0.0, 1.0]`, and waits until playback drains or fails. If no node is configured, PipeWire's current default sink is used. If an explicit node is absent, the call fails rather than leaking speech to another device. Playback errors do not affect TTS model readiness.

**Alternatives considered:** Spawning `pw-play` adds process and PATH dependencies and weakens error handling. Numeric node IDs are unstable across sessions. Returning when queued hides later playback failure from the caller.

### Acquire and observe TTS independently

A TTS registry entry pins the Kokoro int8 ONNX model, required voice archive, immutable upstream revision/release URLs, and SHA-256 values. Acquisition reuses the existing lock, temporary-directory, verification, and atomic-publication policy under an independent TTS cache entry. A configured local override is validated and never silently replaced by a download.

The daemon starts STT and TTS initialization independently after claiming the bus name. The existing STT properties retain their meaning. New read-only properties are `TtsState`, `ActiveTtsProvider`, `ActiveTtsModel`, `TtsDownloadProgress`, `TtsLastError`, `AvailableVoices`, and `TtsCapabilities`; lifecycle changes emit standard `PropertiesChanged`. A TTS failure causes TTS methods to return `ModelUnavailable` while transcription remains usable.

**Alternatives considered:** A unified lifecycle makes an optional output path capable of disabling transcription and makes download progress ambiguous.

### Extend strict startup configuration with a nested TTS section

The optional `tts` mapping contains provider and model identifiers, optional absolute model path, cache override, automatic-download policy, default voice, default speed, optional PipeWire node name, volume, maximum text bytes, reference bytes and seconds, output seconds, and queue capacity. Defaults select the pinned Kokoro int8 model, `af_heart`, speed `1.0`, PipeWire's default sink, volume `1.0`, 16 KiB text, 32 MiB/60 seconds of clone reference audio, 600 seconds of generated output, and queue capacity 8. Unknown or invalid nested fields fail TTS configuration initialization without rewriting values.

The TTS cache defaults beneath the existing XDG Sophon model cache. Configuration remains immutable for the process lifetime.

### Extend stable error mapping for TTS-specific failures

The D-Bus namespace adds `InvalidTtsOptions`, `InvalidReferenceAudio`, `UnsupportedCapability`, `OutputExists`, `OutputFailed`, `SynthesisFailed`, and `PlaybackFailed`. Existing `NotReady`, `ModelUnavailable`, and `ResourceLimit` retain their meanings. Errors are mapped at the service boundary and provider diagnostics are copied into safe error messages.

## Risks / Trade-offs

- [The `tts-rs` ORT requirement does not unify with Sophon's pinned ORT release] → Align the resolved ORT version, disable download/static runtime behavior, and verify Kokoro against the Nix-provided runtime before merging.
- [Kokoro documentation and file discovery accept several model filenames] → Pin one tested int8 artifact and validate the exact manifest instead of accepting arbitrary cache contents.
- [Kokoro requires `espeak-ng` at runtime] → Add it explicitly to the Nix runtime closure and test phonemization from the packaged daemon environment.
- [Direct PipeWire bindings add native build/runtime dependencies] → Add pkg-config discovery, PipeWire libraries, and an isolated playback abstraction with fixture tests; reserve a real PipeWire smoke test for the Nix environment.
- [Buffered synthesis can consume significant memory] → Bound text, queue depth, reference data, and generated duration; keep only owned PCM plus one encoded output per active request.
- [A caller times out while work continues] → Preserve existing worker semantics and document that D-Bus timeout does not cancel accepted synthesis or playback.
- [An existing or concurrently created output path prevents publication after expensive synthesis] → Perform an early existence check for fast failure and authoritative exclusive creation after synthesis; never overwrite.
- [Cloning is exposed before the initial provider supports it] → Advertise capabilities and return `UnsupportedCapability`; test the generic path with a fixture provider so qwentts can be added without changing D-Bus.

## Migration Plan

1. Add dependencies and Nix closure support without changing the D-Bus name, path, or existing transcription methods.
2. Add optional TTS configuration with defaults and independently initialize/download Kokoro on daemon startup.
3. Export TTS methods and properties together so capability-aware clients can distinguish readiness and unsupported modes.
4. Update documentation and integration tests for the expanded introspection contract.
5. Roll back by removing the TTS methods, properties, initialization, and dependencies; existing STT configuration and cached STT models remain valid. Downloaded Kokoro cache data is inert and may be deleted independently.
