## 1. Dependencies and Configuration

- [x] 1.1 Add CPAL with the PipeWire backend and Rubato to the root crate while retaining Hound and the existing system PipeWire build inputs required by CPAL.
- [x] 1.2 Replace `pipewire_node` with optional canonical `output_device` throughout TTS file and operational configuration.
- [x] 1.3 Validate that configured device IDs parse, use the `pipewire` host, and contain a nonempty backend device identifier; add default, valid, malformed, foreign-host, and legacy-field configuration tests.

## 2. Model-Aware STT WAV Normalization

- [x] 2.1 Refactor STT WAV decoding to return source rate and normalized interleaved `f32` PCM for every Hound-supported integer and floating-point PCM representation.
- [x] 2.2 Implement arithmetic-mean multichannel downmixing and reject zero-channel/rate, incomplete-frame, unsupported, malformed, and non-finite inputs with `InvalidAudio`.
- [x] 2.3 Add whole-clip Rubato conversion for mismatched rates with filter-delay and padding trimming, plus an exact equal-rate bypass.
- [x] 2.4 Capture the loaded STT model's advertised nonzero sample rate and route decoded file and descriptor inputs through normalization before the model worker.
- [x] 2.5 Enforce encoded-byte, source-duration, and normalized model-input duration limits around decoding and resampling.
- [x] 2.6 Add WAV fixtures covering supported integer/float depths, mono and multichannel downmixing, up/down sampling, equal-rate identity, malformed inputs, non-finite floats, and resource limits.

## 3. Safe qwentts.cpp Streaming API

- [x] 3.1 Define public owned Qwen stream chunk/control types and a concrete cancellation/callback failure representation without changing buffered `SynthesisResult`.
- [x] 3.2 Implement the native `on_chunk` trampoline with temporary-pointer validation, owned sample copying, `Send` callback state, consumer cancellation, and panic containment.
- [x] 3.3 Add `QwenTtsEngine` streaming synthesis that shares option construction with buffered synthesis, leaves native output empty, and maps native completion, cancellation, and diagnostics correctly.
- [x] 3.4 Add binding-crate tests for chunk order and ownership, unloaded-engine rejection, consumer cancellation, panic containment, option forwarding, and unchanged buffered synthesis behavior.

## 4. Provider and Worker Streaming Contracts

- [x] 4.1 Extend the provider-neutral TTS contract with internal streaming capability and an owned format/chunk/terminal event protocol.
- [x] 4.2 Implement Qwen streaming adapters for default, named, clone, and design modes while preserving their existing validation and buffered synthesis methods.
- [x] 4.3 Keep Kokoro on the buffered fallback path and convert its completed `OwnedAudio` into one logical playback stream for `SpeakAloud`.
- [x] 4.4 Extend the TTS worker with bounded FIFO streaming work items, nonblocking event delivery, sample-budget enforcement, and cancellation propagation while retaining exclusive provider ownership.
- [x] 4.5 Add provider and worker tests for streaming capability selection, ordered early chunks, buffered fallback, queue saturation, duration overflow, cancellation, recovery after failure, and buffered file/buffer routing.

## 5. CPAL PipeWire Playback

- [x] 5.1 Replace the direct PipeWire implementation with a lazy CPAL PipeWire host/device resolver supporting the current default device and exact configured `DeviceId` without fallback.
- [x] 5.2 Implement per-utterance native-rate `f32` output streams that duplicate volume-scaled mono samples across device channels and return `PlaybackFailed` when the requested format cannot open.
- [x] 5.3 Implement the bounded utterance queue and small real-time ring so the CPAL callback performs no blocking or allocation, emits silence on underrun, resumes ordered chunks, and discards unplayed data on failure.
- [x] 5.4 Implement completion tracking that keeps the CPAL stream alive until the final submitted frame's playback deadline and reports stream errors promptly.
- [x] 5.5 Preserve serialized FIFO, non-overlapping aloud operations and coordinate provider completion, playback drain, and bidirectional cancellation in `TtsService::speak_aloud`.
- [x] 5.6 Add playback unit tests for default/exact device policy, missing-device failure, channel duplication, volume, underrun silence, fast-producer buffering, overflow, cancellation, drain completion, serialization, and recovery.

## 6. Integration, Cleanup, and Documentation

- [x] 6.1 Add D-Bus integration tests proving Qwen-style chunks reach playback before synthesis completes, buffered providers still wait, partial streamed failures return stable errors, and file/buffer output remains complete.
- [x] 6.2 Replace the direct PipeWire smoke test with a CPAL-backed isolated PipeWire sink test covering default and exact `pipewire:<node.name>` selection, native-rate opening, and drain behavior.
- [x] 6.3 Remove Sophon's direct `pipewire` Cargo dependency, direct PipeWire imports, obsolete resolver/data structures, and redundant tests while retaining CPAL's transitive system requirements.
- [x] 6.4 Update README configuration, STT input formats, resampling boundary, playback device migration, streaming latency/underrun behavior, partial-audio errors, and dependency wording.
- [x] 6.5 Run formatting, clippy with warnings denied, the full test suite, feature-specific builds/tests, and the CPAL PipeWire smoke harness; resolve all regressions.
