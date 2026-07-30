## Context

`src/tts/mod.rs` currently owns provider contracts and implementations, provider construction, the internal `TtsWorkItem` protocol, the public `TtsStream`, generated-audio validation, `TtsWorker`, and tests for all of them. The project already isolates analogous concerns in `src/stt/worker.rs` and `src/tts/playback.rs`. Other selected changes will alter Qwen dispatch and TTS stream buffering, so isolating scheduling first reduces edit overlap.

The change is structural only. The public crate path, method signatures, channel semantics, worker-thread behavior, and all externally observable synthesis behavior are compatibility constraints.

## Goals / Non-Goals

**Goals:**

- Give TTS scheduling and stream protocol code a focused module.
- Preserve `sophon::tts::{TtsStream, TtsWorker}` and every existing public method signature.
- Preserve provider ownership, FIFO bounded request submission, buffered and streaming routing, validation, cancellation, and error mapping exactly.
- Keep worker tests adjacent to the moved implementation.

**Non-Goals:**

- No queue, channel, stream validation, backpressure, provider, playback, D-Bus, or configuration behavior changes.
- No changes to `TtsProvider`, `TtsRequest`, `TtsStreamEvent`, `TtsStreamControl`, `TtsCapabilities`, or `VoiceIntent`.
- No generalized worker framework shared with STT or playback.
- No dependency or manifest changes.

## Decisions

1. **Create `src/tts/worker.rs` and re-export its public types from `src/tts/mod.rs`.** The before shape defines `TtsStream` and `TtsWorker` directly in `tts::mod`; the after shape defines them in the private module boundary and uses `pub use worker::{TtsStream, TtsWorker};`. This preserves downstream paths while following the existing `stt::worker` convention. Exposing `sophon::tts::worker::*` is not required; the module may remain private unless Rust visibility rules require otherwise.

2. **Move the complete scheduling unit, not isolated fragments.** Move `TtsWorkItem`, `TtsStream` and its methods, `validate_generated_audio`, `TtsWorker` and its methods, and worker-specific test fixtures/tests. Keep `TtsProvider`, all concrete providers, `TtsProviderModel`, `create_tts_provider`, and `validate_capabilities` in provider-oriented code. This avoids a split state machine or circular responsibility.

3. **Preserve imports by symbols rather than source locations.** Verified consumers are `src/tts/service.rs`, `src/tts/playback.rs`, `src/main.rs`, and `tests/dbus_integration.rs`; they SHALL continue importing through `crate::tts` or `sophon::tts`. Internal tests SHALL not rely on private implementation paths unnecessarily.

4. **Apply this change first among TTS Lightness changes.** `simplify-qwen-tts-dispatch` can then focus on provider code, while `bound-tts-stream-handoff` can target `src/tts/worker.rs`. The other changes must still locate symbols rather than assume this ordering so each proposal remains understandable independently.

## Risks / Trade-offs

- **[Risk] A moved item accidentally loses visibility or changes its public path.** → Re-export the exact public types and compile verified consumers without import edits.
- **[Risk] Test movement hides a behavior regression.** → Move tests without weakening assertions and run the complete workspace suite.
- **[Trade-off] One additional module/file is introduced.** → The file maps to an existing scheduling concept and removes mixed responsibilities from the provider module; no new runtime abstraction is added.
- **[Risk] Simultaneous application with other TTS changes causes textual conflicts.** → Apply this change first, then resolve later work by symbol in the new module.

## Migration Plan

1. Move the scheduling unit and worker tests into `src/tts/worker.rs`.
2. Add the module declaration and compatibility re-exports in `src/tts/mod.rs`.
3. Compile all verified call sites without changing their public import paths.
4. Run formatting, Clippy, workspace tests, and the CPU Nix check.

Rollback is a source-only move back into `src/tts/mod.rs`; there is no persisted data, rollout, or external migration.
