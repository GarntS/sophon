## 1. Isolate the scheduling module

- [x] 1.1 Create `src/tts/worker.rs` and move `TtsWorkItem`, `TtsStream`, `validate_generated_audio`, `TtsWorker`, and their required imports without changing logic.
- [x] 1.2 Add the worker module declaration and re-export `TtsStream` and `TtsWorker` from `src/tts/mod.rs` so `sophon::tts::{TtsStream, TtsWorker}` remains unchanged.
- [x] 1.3 Move worker-specific fixtures and tests into the worker module while keeping provider tests with provider code and preserving every existing assertion.

## 2. Verify compatibility boundaries

- [x] 2.1 Verify `src/tts/service.rs`, `src/tts/playback.rs`, `src/main.rs`, and `tests/dbus_integration.rs` compile without changing their public `crate::tts` or `sophon::tts` imports.
- [x] 2.2 Confirm by source review that queue capacity, FIFO scheduling, worker ownership, buffered/streaming routing, validation, cancellation, terminal events, and error variants are unchanged.

## 3. Run project validation

- [x] 3.1 Run `nix develop -c cargo fmt --all -- --check`.
- [x] 3.2 Run `nix develop -c cargo clippy --all-targets -- -D warnings`.
- [x] 3.3 Run `nix develop -c cargo test --workspace`.
- [x] 3.4 Run `nix build .#checks.x86_64-linux.sophon-cpu-qwen-runtime`.
