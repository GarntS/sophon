## Context

Sophon currently decodes STT WAV data in `src/audio.rs` against a hardcoded mono 16 kHz signed-16 contract, queues complete sample vectors to the STT worker, buffers all TTS output into `OwnedAudio`, and implements aloud playback directly against the `pipewire` crate. `SpeakAloud` therefore cannot play Qwen audio until the native call has generated the complete utterance, even though the vendored qwentts.cpp ABI already exposes an incremental audio callback.

The redesign crosses WAV ingestion, model capability propagation, provider contracts, worker scheduling, native FFI safety, real-time playback, configuration, and packaging. Constraints include unchanged D-Bus method signatures, FIFO non-overlapping `SpeakAloud` behavior, startup-only strict configuration, provider-native TTS sample rates, bounded memory, and callbacks that must not block or retain native sample pointers.

## Goals / Non-Goals

**Goals:**

- Use CPAL's PipeWire host for lazily opened, callback-driven speech output.
- Start Qwen playback when its first native audio block is available.
- Preserve one-shot playback for providers without streaming synthesis.
- Preserve buffered Qwen synthesis for file and memfd output.
- Decode common integer and floating-point PCM WAV data for STT, downmix it to mono, and resample only when the active STT model requires another rate.
- Keep all queues and generated audio bounded by existing configuration limits.
- Remove Sophon's direct dependency on and custom use of the `pipewire` crate.

**Non-Goals:**

- Resampling TTS output, clone-reference audio, or any output WAV.
- Changing D-Bus method signatures or adding streaming audio over D-Bus.
- Mixing overlapping utterances or concurrent use of one playback device.
- Supporting compressed input containers.
- Dynamically reloading playback configuration or exposing device enumeration over D-Bus.
- Making non-streaming providers incrementally synthesize audio.

## Decisions

### 1. Use CPAL's explicit PipeWire host and canonical device IDs

Playback will instantiate the CPAL PipeWire host rather than `cpal::default_host()`. An omitted device selects that host's current default output device. A configured `tts.output_device` is parsed as a canonical CPAL `DeviceId`, must use the `pipewire` host prefix, and is resolved with `device_by_id`; a missing device fails without fallback. CPAL's PipeWire ID is backed by the stable PipeWire `node.name`.

This preserves the current default-sink and exact-sink behavior while replacing bespoke registry traversal. A configurable generic CPAL host was rejected because the product requirement is specifically to default to PipeWire and generic hosts would introduce platform-specific configuration and output-rate behavior.

The direct `pipewire` Cargo dependency and direct API usage will be removed. CPAL's `pipewire` feature still brings PipeWire bindings transitively, so the Nix PipeWire development/runtime library remains required.

### 2. Request provider-native mono float playback without Sophon output resampling

Each playback session opens an `f32` CPAL output stream at the provider's sample rate. Mono source samples are multiplied by configured volume and copied to every channel in each device output frame. Sophon will not insert Rubato or any other output resampler. PipeWire may adapt the graph downstream; if CPAL cannot open the requested stream, playback returns `PlaybackFailed`.

Using CPAL's preferred device rate plus application resampling was rejected because it violates the STT-only resampling boundary. Requiring the sink to advertise the provider rate before attempting a stream was also rejected because PipeWire can often accept and graph-convert an explicitly requested rate.

### 3. Use a two-stage bounded stream between synthesis and the real-time callback

Streaming aloud playback uses this flow:

```text
native/provider callback -> utterance queue -> small playback ring -> CPAL callback
```

The provider callback copies each temporary native block into owned Rust storage and performs a nonblocking enqueue. The logical utterance queue tracks its total sample budget and rejects data beyond `sample_rate * max_generated_audio_seconds`, so a fast generator can run ahead of real-time playback without unbounded memory. A small real-time ring isolates queue synchronization from CPAL's callback. The CPAL callback never blocks and writes silence when synthesis temporarily underruns.

Playback starts as soon as the first nonempty chunk establishes the stream format. There is no startup prebuffer. A terminal success causes playback to drain all accepted samples before replying. A synthesis, queue, or playback failure stops the session, discards unplayed samples, and reports the corresponding stable error. Audio already submitted to the device cannot be recalled, so a failed streamed call can have produced partial audible speech.

A tiny queue that aborts whenever generation outruns playback was rejected because fast Qwen configurations would fail routinely. Blocking the native callback was rejected because qwentts.cpp can invoke callbacks from a shared batching thread in future configurations.

### 4. Add streaming as an optional provider operation, not a replacement for buffered synthesis

The provider-neutral contract will distinguish buffered synthesis from streaming synthesis support. `SpeakToFile` and `SpeakToBuffer` always use buffered synthesis. `SpeakAloud` requests streaming when advertised; otherwise the worker synthesizes one `OwnedAudio` and submits it as a single logical stream.

The TTS worker remains the exclusive owner of the mutable provider. A streaming work item carries an event sink and final result channel. It emits format/chunk/terminal events while the synchronous provider call runs. The service concurrently awaits provider completion and playback completion. Existing bounded FIFO inference and non-overlapping aloud semantics remain intact.

Using one streaming path for file and memfd output was rejected because qwentts.cpp's streaming codec path is not necessarily sample-identical to its buffered codec path and those endpoints promise complete provider-native WAV output.

### 5. Wrap qwentts.cpp streaming with owned chunks and contained callbacks

`qwentts-cpp` will expose a safe synchronous streaming method alongside `synthesize`. It will configure `qt_tts_params.on_chunk`, copy every native `f32` slice into an owned chunk before invoking Rust code, prevent native pointer escape, catch Rust panics at the C boundary, and map callback-requested cancellation separately from ordinary native synthesis failures. The callback contract will be `Send` because native batching can invoke it from an internal worker thread and must not call back into the engine.

Buffered synthesis and owned `SynthesisResult` remain unchanged. This keeps unsafe callback state and lifetime handling inside the binding crate rather than in Sophon.

### 6. Make STT WAV normalization model-aware

WAV ingestion will return decoded metadata and normalized interleaved `f32` PCM instead of enforcing 16 kHz during parsing. It will accept Hound-supported integer and floating-point PCM with one or more channels, reject malformed data, zero channels, invalid rates, unsupported encodings, incomplete frames, and non-finite float samples, and compute each mono frame as the arithmetic mean of its channels. Averaging avoids gain growth and does not require channel-layout metadata that RIFF/WAVE may omit.

The active model's `SpeechModel::capabilities().sample_rate` is captured during STT initialization and propagated to the STT service. If the source and model rates differ, a Rubato whole-clip resampler processes the mono buffer and trims filter delay and padding. Equal rates bypass Rubato exactly. Encoded-size limits are checked before full decoding where the source permits, and decoded duration is checked using both source frame/rate metadata and normalized model-input frames so malformed metadata or conversion rounding cannot bypass the configured limit.

Resampling in the D-Bus layer or hardcoding 16 kHz was rejected because the model owns the input-rate contract. Streaming STT resampling was rejected because both transcription methods already accept complete seekable WAV inputs and the model worker consumes a complete sample slice.

### 7. Keep playback acquisition lazy and failure-isolated

The CPAL device and stream are opened for each queued aloud operation on the dedicated playback thread. This re-resolves the current default sink, permits device changes between calls, and prevents an absent playback device from making `SpeakToFile` or `SpeakToBuffer` unavailable. A configured missing device and stream errors affect only the aloud call and do not change TTS model readiness.

A daemon-lifetime CPAL stream was rejected because it would pin startup device state and couple independent synthesis availability to audio hardware availability.

## Risks / Trade-offs

- **[Streaming synthesis can fail after speech becomes audible]** → Document partial-audio semantics, stop promptly, discard unplayed data, and retain the stable final error.
- **[A slower-than-real-time provider causes gaps]** → CPAL writes silence on underrun and resumes on the next chunk; immediate latency is intentionally prioritized over prebuffering.
- **[A fast provider can retain much of an utterance in memory]** → Charge every queued sample against the configured generated-duration limit; the default ten-minute 24 kHz mono `f32` ceiling is approximately 55 MiB.
- **[CPAL has no universal explicit drain primitive]** → Track the final submitted frame and keep the stream alive through its playback deadline using callback timing/rate information before reporting success.
- **[Device IDs are backend-specific]** → Require the canonical `pipewire:` prefix, validate strictly at startup, and document migration from `pipewire_node`.
- **[Broad PCM support creates conversion edge cases]** → Centralize decoding and normalization, test every supported integer/float width, reject non-finite values and incomplete channel frames, and use arithmetic-mean downmixing.
- **[Rubato filtering slightly changes STT waveforms]** → Bypass equal rates and use a deterministic high-quality fixed-ratio whole-clip configuration with delay trimming.
- **[CPAL still transitively depends on PipeWire bindings]** → Describe dependency removal precisely: Sophon drops direct API ownership while packaging retains the system PipeWire library required by CPAL.

## Migration Plan

1. Add CPAL with its PipeWire backend and Rubato while retaining the old playback implementation behind the implementation sequence.
2. Introduce model-aware STT decode/normalize/resample behavior and update tests and documentation.
3. Add and test the safe qwentts.cpp streaming API.
4. Introduce the provider-neutral stream events and CPAL playback worker, then route `SpeakAloud` through them.
5. Replace `tts.pipewire_node` with `tts.output_device`; users migrate `node.name = X` to `output_device: pipewire:X`.
6. Replace the direct PipeWire smoke harness with a CPAL-backed isolated PipeWire sink test.
7. Remove direct PipeWire code and dependency declarations after all callers have moved.

Rollback is a source/package rollback. Configuration rollback requires changing `tts.output_device: pipewire:X` back to `tts.pipewire_node: X` because strict parsing intentionally rejects fields from the other version.
