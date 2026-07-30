## MODIFIED Requirements

### Requirement: Bounded synthesis workload
The service SHALL enforce configured nonzero limits for UTF-8 text bytes, generated audio duration, reference audio, and queued synthesis requests. Streamed synthesis SHALL additionally use a fixed handoff budget independent of the generated-duration limit: handed-off chunks SHALL contain at most 4,096 mono samples, the worker-to-consumer audio channel SHALL retain at most four events, and a producer SHALL wait for consumer capacity rather than accumulating beyond that bound. The defaults SHALL be 16 KiB of text, 600 seconds of generated output, 32 MiB and 60 seconds of reference audio, and queue capacity 8.

#### Scenario: Text exceeds its limit
- **WHEN** the UTF-8 encoding of request text exceeds the configured text limit
- **THEN** the method returns `ResourceLimit` without queueing inference

#### Scenario: Empty text is rejected
- **WHEN** request text is empty or contains only whitespace
- **THEN** the method returns `InvalidTtsOptions` without queueing inference

#### Scenario: Buffered generated output exceeds its limit
- **WHEN** a provider returns buffered audio exceeding the configured generated-duration limit
- **THEN** no file, memfd, or playback output is published and the method returns `ResourceLimit`

#### Scenario: Streamed generated output exceeds its limit
- **WHEN** a streaming provider emits audio that would exceed the configured generated-duration limit
- **THEN** Sophon cancels further generation, stops playback, discards unplayed samples, and returns `ResourceLimit`, while audio already played remains audible

#### Scenario: Stream consumer is slower than generation
- **WHEN** four bounded handoff events are retained and the provider emits more valid samples
- **THEN** the provider worker waits until the consumer frees capacity and then resumes without dropping, duplicating, reordering, or resampling samples

#### Scenario: Stream consumer is dropped while producer waits
- **WHEN** a stream consumer closes while the provider callback is waiting for handoff capacity
- **THEN** the callback unblocks, requests provider cancellation, terminates that request, and leaves the worker available for a later request

#### Scenario: Queue is full
- **WHEN** the bounded TTS inference queue has no capacity
- **THEN** a new otherwise-valid request returns `ResourceLimit`
