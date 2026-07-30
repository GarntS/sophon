## MODIFIED Requirements

### Requirement: Bounded synthesis workload
The service SHALL enforce configured nonzero limits for UTF-8 text bytes, generated audio duration, reference audio, queued synthesis requests, and queued streamed audio. The defaults SHALL be 16 KiB of text, 600 seconds of generated output, 32 MiB and 60 seconds of reference audio, and queue capacity 8.

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

#### Scenario: Queue is full
- **WHEN** the bounded TTS inference queue has no capacity
- **THEN** a new otherwise-valid request returns `ResourceLimit`

## ADDED Requirements

### Requirement: Stream-capable aloud synthesis
`SpeakAloud` SHALL use incremental provider output when the active provider advertises streaming synthesis and SHALL preserve buffered synthesis for providers without that capability. `SpeakToFile` and `SpeakToBuffer` SHALL continue to use complete buffered provider output regardless of streaming capability.

#### Scenario: Streaming provider speaks aloud
- **WHEN** a valid `SpeakAloud` request uses a provider that supports streaming synthesis
- **THEN** provider chunks are offered to playback in generation order before synthesis completes

#### Scenario: Buffered provider speaks aloud
- **WHEN** a valid `SpeakAloud` request uses a provider without streaming synthesis
- **THEN** playback begins after the provider returns its complete audio result

#### Scenario: Streaming provider writes a file
- **WHEN** `SpeakToFile` or `SpeakToBuffer` uses a provider that also supports streaming synthesis
- **THEN** the endpoint obtains and publishes the complete buffered provider result rather than streamed playback chunks

### Requirement: Partial-audio streamed failure semantics
A streamed `SpeakAloud` call SHALL report a synthesis, resource, or playback error even when earlier chunks have already become audible, SHALL stop accepting new chunks, and SHALL discard chunks not yet submitted to the device.

#### Scenario: Synthesis fails after first audio
- **WHEN** a streaming provider emits audible chunks and then fails synthesis
- **THEN** `SpeakAloud` stops playback as promptly as possible and returns `SynthesisFailed`; already played audio is not treated as a successful call

#### Scenario: Playback fails during synthesis
- **WHEN** playback fails while the provider is still generating
- **THEN** Sophon requests synthesis cancellation, discards unplayed chunks, and returns `PlaybackFailed`
