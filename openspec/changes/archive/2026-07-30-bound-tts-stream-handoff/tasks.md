## 1. Add bounded stream transport

- [x] 1.1 In the TTS worker implementation (`src/tts/worker.rs` after the recommended module-isolation change, otherwise the current worker section), define 4,096-sample chunk and four-event channel limits.
- [x] 1.2 Replace the production unbounded event channel with a bounded Tokio MPSC channel for format/audio events and a separate oneshot terminal result, without changing public `TtsStream` method signatures.
- [x] 1.3 Implement `TtsStream::next`, `blocking_next`, and crate-private polling so public consumers receive format and ordered chunks before exactly one terminal event, while playback can observe a ready terminal error promptly.
- [x] 1.4 Ensure dropping either stream receiver closes the handoff, unblocks any waiting producer send, and allows the worker to process later requests.

## 2. Encapsulate validation and chunk handoff

- [x] 2.1 Extract stream format, event order, finite-sample, duration, reserved-terminal, and nonempty-output checks into one private validator with explicit finish validation.
- [x] 2.2 Add one handoff helper that ignores empty chunks and sends consecutive chunks of at most 4,096 samples through bounded blocking sends.
- [x] 2.3 Route native streaming callbacks through the validator and handoff helper, retaining the first validation/send failure as the terminal cause and returning `TtsStreamControl::Cancel`.
- [x] 2.4 Route buffered-provider fallback through the same chunk splitter and bounded channel while preserving its complete-audio validation and format-first ordering.

## 3. Bound playback-owned pending audio

- [x] 3.1 Reorder `CpalPlayback::play` to fill the ring before receiving more samples and to retain at most one nonempty 4,096-sample chunk outside the ring.
- [x] 3.2 Poll the independent terminal result before submitting additional queued samples; on terminal error, discard unplayed channel, pending, and ring data and return the original error.
- [x] 3.3 Preserve successful drain detection, provider-native sample rate, output-device selection, volume, silence-on-underrun, FIFO order, and the allocation/blocking-free CPAL callback.

## 4. Add deterministic bound and compatibility tests

- [x] 4.1 Add a fast-producer/idle-consumer test proving production pauses after the four-event handoff fills and resumes in order as the consumer drains it.
- [x] 4.2 Add a dropped-consumer-while-blocked test proving cancellation unblocks generation and a subsequent buffered or streamed request succeeds.
- [x] 4.3 Add native-stream and buffered-fallback tests proving large chunks are split to at most 4,096 samples and concatenate to the exact original sample sequence.
- [x] 4.4 Add public `next` and `blocking_next` tests proving format/chunk/terminal order remains unchanged with the separate terminal signal.
- [x] 4.5 Add playback tests proving at most one chunk remains outside the ring and a terminal failure discards unplayed audio promptly even when the bounded audio channel is full.
- [x] 4.6 Retain and pass existing overflow, partial-failure, early-streaming, FIFO, queue-full, playback serialization, ring, deadline, and recovery tests.

## 5. Run project validation

- [x] 5.1 Verify `rg -n 'unbounded_channel' src/tts` finds no production TTS stream channel (test-only helpers may remain only if they cannot mask production behavior).
- [x] 5.2 Run `nix develop -c cargo fmt --all -- --check`.
- [x] 5.3 Run `nix develop -c cargo clippy --all-targets -- -D warnings`.
- [x] 5.4 Run `nix develop -c cargo test --workspace`.
- [x] 5.5 Run `nix build .#checks.x86_64-linux.sophon-cpu-qwen-runtime`.
- [x] 5.6 Run `tests/pipewire-smoke.sh` inside `nix develop` to verify native-rate playback still drains through the bounded handoff.
