## Context

`TtsWorker` uses a bounded synchronous queue for requests but creates a Tokio unbounded channel for every accepted stream. Its provider callback validates emitted events and sends them immediately. `CpalPlayback::play` repeatedly drains all currently available channel events into a `VecDeque` before filling a fixed 4,096-frame `SampleRing`. Consequently, the channel and playback queue can retain most of a permitted utterance; configuration permits 3,600 generated seconds.

The user authorized changing the existing nonblocking-provider requirement to bounded backpressure. Early playback, FIFO samples, duration enforcement, prompt terminal failure, dropped-consumer cancellation, public `TtsStream` behavior, and a nonblocking/allocation-free CPAL callback remain constraints.

## Goals / Non-Goals

**Goals:**

- Bound service-owned queued stream audio to 24,576 mono samples per accepted stream.
- Apply backpressure on the synchronous provider worker outside the real-time output callback.
- Preserve public stream event order and prompt failure/cancellation semantics.
- Split arbitrarily large provider/fallback events into bounded handoff chunks.
- Replace nested parallel validation state with one private stream validator.

**Non-Goals:**

- No new configuration or dependency.
- No changes to `TtsProvider::synthesize_streaming`, `TtsStreamEvent`, `TtsStreamControl`, or public `TtsStream` method signatures.
- No change to maximum generated duration, provider-native sample rates, playback resampling policy, inference queue capacity, or serialized `SpeakAloud` behavior.
- No attempt to bound memory allocated internally by a provider before it invokes the callback, or complete buffered audio already returned by a non-streaming provider.
- No blocking, locking beyond the existing try-lock, or allocation in the CPAL output callback.

## Decisions

1. **Use fixed frame and channel budgets.** Define private constants equivalent to `STREAM_CHUNK_SAMPLES = 4_096` and `STREAM_CHANNEL_EVENTS = 4`. Every handed-off `Chunk` contains at most 4,096 mono samples. The channel can therefore retain at most 16,384 samples; playback retains at most one 4,096-sample pending chunk and the existing ring retains 4,096 frames, for an end-to-end service-owned queued-audio ceiling of 24,576 samples. Empty chunks are ignored as today. These are internal constants, not configuration surface.

2. **Use a bounded Tokio MPSC audio channel and a separate terminal oneshot.** `TtsStream` changes internally from one unbounded event receiver to a bounded receiver for `Format`/`Chunk` plus a one-shot terminal result. Its existing `next`, `blocking_next`, and crate-private `try_next` methods continue yielding `Format`, ordered `Chunk`s, then exactly one `Terminal` event. Separating terminal status prevents a full audio channel from hiding a synthesis failure behind buffered samples and lets playback stop promptly.

3. **Block only the dedicated provider worker.** The synchronous provider emit callback uses `blocking_send` for format and bounded chunk events. If the channel is full, generation waits until the consumer drains capacity. A closed receiver converts to the existing consumer-cancelled `SynthesisFailed` path and returns `TtsStreamControl::Cancel`, so provider work and queue service recover. No async executor or CPAL callback is blocked.

4. **Split events before handoff without changing sample order.** Validate the provider's complete emitted chunk, then send consecutive owned slices of no more than 4,096 samples. The buffered-provider fallback uses the same splitter for its complete `OwnedAudio`. No sample is duplicated, omitted, reordered, or resampled. The current provider-owned chunk may exist while its pieces are sent; the bound applies to service-owned queued handoff copies, not provider allocation.

5. **Model stream protocol validation explicitly.** Introduce a private validator with `sample_rate: Option<u32>`, `accepted_samples: u128`, and `max_generated_audio_seconds`. It accepts one provider event, enforcing one nonzero format before audio, finite samples, total duration, and rejection of provider-emitted terminal events; `finish` requires a format and at least one accepted sample. The first validation/send failure is retained as the terminal cause and requests cancellation. This replaces the nested `sample_rate`, `accepted_samples`, and `callback_failure` locals without changing error text or classifications.

6. **Prevent playback from defeating channel backpressure.** Reorder `CpalPlayback::play` so it fills the ring from its current pending chunk before receiving more audio and stops receiving after retaining one nonempty chunk outside the ring. It may continue consuming zero-sample/control events. It polls the separate terminal result each loop: an error immediately discards pending/channel audio and returns; success still waits for all ordered audio and the latest device deadline to drain.

7. **Retain public-consumer terminal ordering.** General `TtsStream::next` and `blocking_next` drain audio-channel events before exposing the terminal result, matching the existing public sequence. The playback-specific crate-private terminal poll exists only to preserve prompt failure semantics. Dropping `TtsStream` drops both receivers and unblocks a producer waiting on capacity.

## Risks / Trade-offs

- **[Trade-off] A consumer that holds a stream without consuming or dropping it can pause the sole TTS provider worker.** → This is the authorized backpressure behavior; document it in tests and ensure dropping the stream unblocks and recovers the worker.
- **[Risk] A separate terminal signal changes public event ordering.** → Public receive methods SHALL synthesize `Terminal` only after the audio channel is drained; add explicit ordering tests.
- **[Risk] Playback drains too many events into its local queue.** → Enforce and test the one-nonempty-pending-chunk invariant.
- **[Risk] A failure is delayed behind backpressured audio.** → Poll the independent terminal result before filling or submitting additional audio and discard unplayed samples on error.
- **[Risk] Chunk splitting changes output.** → Compare concatenated emitted/played samples byte-for-value and retain format exactly once.
- **[Trade-off] More channel plumbing is introduced.** → It replaces unbounded storage, makes terminal priority explicit, and keeps the public protocol unchanged.

## Migration Plan

1. Apply `isolate-tts-worker-module` first when following the recommended order.
2. Add constants, the validator, bounded audio sender/receiver, separate terminal result, and chunk splitter in the worker module/current worker section.
3. Update streaming-provider and buffered-fallback routing to use the shared bounded sender.
4. Update `TtsStream` receive methods with unchanged signatures and event ordering.
5. Bound playback pending audio and add prompt terminal polling without touching the CPAL callback.
6. Add deterministic fast-producer, idle-consumer, split-chunk, terminal-order, partial-failure, dropped-consumer, fallback, and recovery tests.
7. Run standard Rust, integration, Nix CPU, and PipeWire smoke validation.

Rollback restores the unbounded event channel and previous playback drain loop. No data or deployment migration is required, but rollback restores the former nonblocking-provider contract.
