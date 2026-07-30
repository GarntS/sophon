## Why

TTS requests enter a bounded inference queue, but each accepted stream uses an unbounded event channel and playback can drain those events into another growing queue before feeding its 4,096-frame ring. A fixed sample budget with backpressure makes accepted-stream memory independent of the configured maximum utterance duration and makes the stream state machine explicit.

## What Changes

- Replace the production unbounded TTS event channel with a bounded channel carrying chunks of at most 4,096 mono samples.
- Bound the worker-to-consumer channel to four events and bound playback-owned pending audio to one 4,096-sample chunk, while retaining the existing 4,096-frame CPAL ring; service-owned queued handoff audio is therefore at most 24,576 mono samples.
- Split native and buffered-fallback output chunks as necessary before handoff, preserving sample order and terminal semantics.
- Permit the synchronous provider callback to wait when the handoff is full and resume as playback consumes samples; dropping or failing the consumer still cancels generation and unblocks the producer.
- Extract the nested worker stream-format/order/duration checks into a private stream-validation state component.
- **BREAKING**: replace the existing requirement that a fast producer callback never blocks with an explicitly bounded backpressure contract, as authorized during audit deep-dive.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `speech-synthesis`: Define a fixed bound and backpressure behavior for queued streamed audio while preserving synthesis limits and errors.
- `speech-playback`: Replace duration-sized nonblocking accumulation with a fixed end-to-end service-owned playback handoff bound.

## Impact

- Primary symbols: `TtsStream`, `TtsWorkItem`, `TtsWorker::new`, `TtsWorker::synthesize_streaming`, and streaming validation in current `src/tts/mod.rs` or `src/tts/worker.rs` after `isolate-tts-worker-module`.
- Playback symbols: `CpalPlayback::play`, `SampleRing`, `fill_ring_from_chunks`, and playback tests in `src/tts/playback.rs`.
- Provider callback path: `TtsProvider::synthesize_streaming` implementations and fixtures; no public signature changes.
- Verified integration behavior: `TtsService::speak_aloud`, `tests/dbus_integration.rs`, Qwen native cancellation, buffered-provider fallback, partial failures, and playback draining.
- No new configuration, dependency, D-Bus method, public Rust signature, audio format, persisted-data, or native bridge changes.
- Ordering: apply `isolate-tts-worker-module` first; this change is otherwise specified by symbols so its intent remains clear if paths differ.
