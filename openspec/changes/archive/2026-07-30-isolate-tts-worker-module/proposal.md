## Why

`src/tts/mod.rs` currently mixes provider contracts and implementations with stream protocol and worker scheduling, while STT and playback already keep scheduling in focused modules. Isolating the TTS worker follows the established project structure and reduces navigation and coordination cost before the other selected TTS refactors.

## What Changes

- Move `TtsWorkItem`, `TtsStream`, `validate_generated_audio`, and `TtsWorker` from `src/tts/mod.rs` into a new `src/tts/worker.rs` module.
- Re-export `TtsStream` and `TtsWorker` from `sophon::tts` so every existing public Rust path and signature remains unchanged.
- Move worker-focused unit tests with the implementation while leaving provider-focused tests with provider code.
- Make no runtime, protocol, queueing, error, configuration, or D-Bus behavior changes.

## Capabilities

### New Capabilities

- `tts-worker-compatibility`: Public Rust API and behavioral compatibility constraints for isolating TTS scheduling internals.

### Modified Capabilities

None. Existing speech synthesis and playback requirements remain unchanged.

## Impact

- Primary implementation locations: `src/tts/mod.rs` and new `src/tts/worker.rs`.
- Verified call sites that must continue compiling unchanged: `src/tts/service.rs`, `src/tts/playback.rs`, `src/main.rs`, `tests/dbus_integration.rs`, and existing `src/tts/mod.rs` tests.
- Public compatibility boundary: `sophon::tts::{TtsStream, TtsWorker}` and all methods on those types.
- No dependency, manifest, configuration, persisted-data, native-provider, or external API changes.
- Recommended implementation order: apply this change before `simplify-qwen-tts-dispatch` and `bound-tts-stream-handoff` to minimize overlapping edits in `src/tts/mod.rs`.
