## ADDED Requirements

### Requirement: Session D-Bus synthesis interface
The system SHALL export `SpeakToFile`, `SpeakToBuffer`, and `SpeakAloud` on interface `com.garntresearch.sophon` at `/com/garntresearch/sophon` on the user session bus without changing the existing transcription methods.

#### Scenario: Introspection exposes synthesis methods
- **WHEN** a client introspects the Sophon object
- **THEN** the interface lists `SpeakToFile(s, s, a{sv}) -> t`, `SpeakToBuffer(s, a{sv}) -> (h, t)`, and `SpeakAloud(s, a{sv})`

### Requirement: Complete provider-native WAV output
File and buffer synthesis SHALL encode complete mono RIFF/WAVE audio using the provider's sample rate and 32-bit IEEE-float PCM samples. The initial Kokoro provider output SHALL therefore be mono 24 kHz float WAV.

#### Scenario: Successful synthesis produces a complete WAV
- **WHEN** a ready provider successfully synthesizes valid text
- **THEN** the file or descriptor output contains a finalized WAV whose frames represent the complete synthesis result

### Requirement: Exclusive file synthesis
`SpeakToFile` SHALL accept non-empty text, an absolute destination path, and an options dictionary, and SHALL create and write the destination only if it does not already exist. It SHALL return the encoded byte length and SHALL never truncate or replace an existing filesystem object.

#### Scenario: New destination is written
- **WHEN** a client supplies a valid request and an absolute destination that remains absent through exclusive creation
- **THEN** the method creates a complete WAV and returns its byte length

#### Scenario: Existing destination is rejected
- **WHEN** the destination exists before or is created concurrently with publication
- **THEN** the method returns `OutputExists` and leaves the existing object unchanged

#### Scenario: Output writing fails
- **WHEN** an exclusively created destination cannot be completely written or finalized
- **THEN** the method returns `OutputFailed` and removes the partial destination it created

### Requirement: Immutable server-created buffer synthesis
`SpeakToBuffer` SHALL create a Linux memfd, write and finalize the complete WAV, rewind it to byte zero, make its size and content immutable with file seals, and return the transferred descriptor with its encoded byte length.

#### Scenario: Client receives synthesized memfd
- **WHEN** buffer synthesis succeeds
- **THEN** the client receives a readable descriptor positioned at byte zero and a byte length matching the complete WAV

#### Scenario: Returned memfd is immutable
- **WHEN** a client attempts to write, grow, or shrink a returned synthesis descriptor
- **THEN** the kernel rejects the modification while reads remain available until the client releases its descriptor

### Requirement: Strict per-request synthesis options
All synthesis methods SHALL recognize `voice` as a string, `language` as a string, `speed` as a double, `clone_audio` as a transferred Unix descriptor, `clone_transcript` as a string, and `voice_description` as a string. Omitted supported values SHALL use configured defaults. Named voice, cloning, and voice description SHALL be mutually exclusive intents, and clone transcript SHALL require clone audio.

#### Scenario: Named voice is selected
- **WHEN** a client supplies a voice name advertised by the active model
- **THEN** that voice is used for the request without reloading the model

#### Scenario: One-shot clone is requested
- **WHEN** a client supplies valid `clone_audio` and an optional `clone_transcript` to a provider advertising cloning
- **THEN** the reference is used for that synthesis call and is not persisted as a named voice

#### Scenario: Voice design is requested
- **WHEN** a client supplies `voice_description` to a provider advertising voice design
- **THEN** the description is used as the request's voice intent

#### Scenario: Options are invalid
- **WHEN** options contain an unknown key, wrong type, unsupported voice, contradictory voice intents, orphan clone transcript, invalid speed, or incompatible language and voice
- **THEN** the method returns `InvalidTtsOptions` without queueing inference

#### Scenario: Capability is unsupported
- **WHEN** a valid request uses cloning, voice design, or another operation not advertised by the active provider
- **THEN** the method returns `UnsupportedCapability` without silently substituting a different voice intent

### Requirement: Canonical one-shot clone reference audio
A transferred clone descriptor SHALL be readable and seekable from byte zero and contain a complete mono 24 kHz 32-bit IEEE-float WAV within configured encoded-byte and decoded-duration limits.

#### Scenario: Canonical reference is accepted
- **WHEN** a capable provider receives canonical reference audio within both limits
- **THEN** Sophon decodes its owned `f32` samples and submits the clone intent

#### Scenario: Invalid reference is rejected
- **WHEN** reference data is malformed, non-seekable, incomplete, or has another channel count, sample rate, sample encoding, or container
- **THEN** the method returns `InvalidReferenceAudio` before synthesis

#### Scenario: Oversized reference is rejected
- **WHEN** reference audio exceeds a configured byte or duration limit
- **THEN** the method returns `ResourceLimit` before synthesis

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

### Requirement: Readiness-aware synthesis
Synthesis SHALL only be queued while the independent TTS lifecycle is `Ready`.

#### Scenario: TTS is initializing
- **WHEN** a synthesis method is called while TTS is initializing, downloading, or loading
- **THEN** it returns retryable `NotReady`

#### Scenario: TTS initialization failed
- **WHEN** a synthesis method is called after TTS initialization failed
- **THEN** it returns `ModelUnavailable` while transcription remains governed by its independent STT state

### Requirement: Stable synthesis errors
The D-Bus interface SHALL expose `InvalidTtsOptions`, `InvalidReferenceAudio`, `UnsupportedCapability`, `OutputExists`, `OutputFailed`, `SynthesisFailed`, and `PlaybackFailed` under the Sophon error namespace, and SHALL continue using `NotReady`, `ModelUnavailable`, and `ResourceLimit` where applicable.

#### Scenario: Provider inference fails
- **WHEN** the active provider fails during a valid accepted request
- **THEN** the caller receives `SynthesisFailed` with a safe diagnostic and the daemon remains available for later requests

### Requirement: Discoverable synthesis speed support
The active TTS provider SHALL advertise whether it supports speed control. Kokoro SHALL advertise `speed-control`; Qwen providers SHALL NOT advertise it and SHALL accept only unit speed.

#### Scenario: Client inspects Kokoro capabilities
- **WHEN** Kokoro is ready
- **THEN** `TtsCapabilities` includes `speed-control` and valid configured or per-request speed affects synthesis

#### Scenario: Client requests non-unit Qwen speed
- **WHEN** a Qwen provider is selected and configured or requested speed differs from `1.0`
- **THEN** Sophon returns `InvalidTtsOptions` before queueing inference rather than ignoring the value

### Requirement: Bounded text-like synthesis inputs
Sophon SHALL independently apply the configured maximum text-byte limit to synthesis text, clone transcripts, per-request voice descriptions, and configured default voice descriptions. Consumed descriptions and transcripts SHALL be non-empty after trimming and SHALL reject interior NUL and non-whitespace control characters.

#### Scenario: Clone transcript exceeds the limit
- **WHEN** clone audio is valid but its supplied transcript exceeds `max_text_bytes`
- **THEN** Sophon returns `ResourceLimit` before extracting a voice reference or queueing inference

#### Scenario: Voice description contains invalid controls
- **WHEN** a request or configured default description contains NUL or a non-whitespace control character
- **THEN** TTS validation fails before native synthesis

#### Scenario: Each input independently fits
- **WHEN** synthesis text and a voice description each fit the configured limit
- **THEN** they remain valid even if their combined byte length exceeds one limit

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
