## ADDED Requirements

### Requirement: Session D-Bus transcription interface
The system SHALL own `com.garntresearch.sophon` on the user session bus and SHALL export interface `com.garntresearch.sophon` at `/com/garntresearch/sophon` with `TranscribeFile` and `TranscribeMemfd` methods.

#### Scenario: Introspection exposes the transcription methods
- **WHEN** a client introspects `/com/garntresearch/sophon`
- **THEN** the `com.garntresearch.sophon` interface lists `TranscribeFile` and `TranscribeMemfd` with audio input, options dictionary, and text output arguments

### Requirement: File transcription
The `TranscribeFile` method SHALL accept an absolute path to a regular WAV file and an options dictionary, SHALL perform one transcription operation, and SHALL return the final transcribed text in the method reply. Regular-file type, encoded size, and WAV content SHALL be evaluated from the same opened filesystem object. Opening a non-regular object SHALL NOT wait for that object's data source.

#### Scenario: Valid file is transcribed
- **WHEN** a client calls `TranscribeFile` with an accessible absolute path containing supported WAV audio and valid options while the model is ready
- **THEN** the method returns the transcription as a string

#### Scenario: Relative file path is rejected
- **WHEN** a client calls `TranscribeFile` with a relative path
- **THEN** the method returns an `InvalidAudio` error without queueing inference

#### Scenario: Non-regular file is rejected
- **WHEN** the path identifies an object other than a regular file
- **THEN** the method returns an `InvalidAudio` error without waiting for a FIFO writer or queueing inference

#### Scenario: Path changes after opening
- **WHEN** an accepted pathname is replaced after Sophon opens it but before validation or parsing completes
- **THEN** regular-file validation, encoded-size enforcement, and WAV parsing all apply to the originally opened object rather than resolving the pathname again

#### Scenario: Symlink resolves to a regular file
- **WHEN** an absolute input path is a symlink whose target is an accessible regular WAV file
- **THEN** Sophon validates and transcribes the opened target under the same size, duration, and WAV rules as a direct path

### Requirement: Unix-FD transcription
The `TranscribeMemfd` method SHALL accept a transferred readable, seekable Unix file descriptor containing a complete WAV file and an options dictionary, SHALL read from offset zero, and SHALL return the final transcribed text. The descriptor SHALL NOT be required to identify a literal Linux memfd.

#### Scenario: Valid memfd is transcribed
- **WHEN** a client transfers a seekable descriptor containing supported WAV audio while the model is ready
- **THEN** the service reads the WAV from offset zero and returns the transcription as a string

#### Scenario: Non-seekable descriptor is rejected
- **WHEN** a client transfers a descriptor that cannot seek to offset zero
- **THEN** the method returns an `InvalidAudio` error without queueing inference

### Requirement: Canonical WAV input
The service SHALL accept complete RIFF/WAVE audio containing one or more channels of Hound-supported integer or IEEE-floating-point PCM, SHALL normalize samples to finite `f32`, SHALL downmix each multichannel frame to the arithmetic mean of its channels, and SHALL resample the mono audio to the active model's advertised input sample rate when necessary. It SHALL reject malformed WAV data, zero channels or sample rate, unsupported encodings, incomplete frames, and non-finite samples.

#### Scenario: Model-rate mono WAV is accepted
- **WHEN** input is a valid supported mono PCM WAV at the active model's advertised input rate and within resource limits
- **THEN** normalized samples are submitted for transcription without resampling

#### Scenario: Integer PCM is normalized
- **WHEN** input uses a supported signed or unsigned integer PCM representation
- **THEN** samples are normalized to finite `f32` values before downmixing or transcription

#### Scenario: Floating-point PCM is normalized
- **WHEN** input uses supported finite IEEE-floating-point PCM
- **THEN** its values are decoded as `f32` samples without integer quantization

#### Scenario: Multichannel input is downmixed
- **WHEN** a valid WAV contains multiple channels
- **THEN** each output mono frame is the arithmetic mean of the corresponding source-channel samples

#### Scenario: Input rate differs from model rate
- **WHEN** valid decoded audio has a sample rate different from the active model's nonzero advertised rate
- **THEN** the mono samples are resampled to that model rate before inference

#### Scenario: Audio is malformed or unsupported
- **WHEN** WAV data is malformed, has no channels, has a zero rate, uses an unsupported encoding, ends with an incomplete frame, or contains a non-finite sample
- **THEN** the method returns `InvalidAudio` without queueing inference

### Requirement: Per-request transcription options
Both transcription methods SHALL accept an `a{sv}` options dictionary recognizing only `language` as a string. An omitted language SHALL use the configured default, and the selected language SHALL be validated against registry metadata for the active model.

#### Scenario: Request language overrides the default
- **WHEN** a client supplies a language supported by the active model
- **THEN** that language is used for the request without reloading the model

#### Scenario: Invalid option is rejected
- **WHEN** options contain an unknown key, a value of the wrong type, or an unsupported language
- **THEN** the method returns `InvalidOptions` without queueing inference

### Requirement: Bounded short-recording workload
The service SHALL enforce configurable encoded-size, decoded-duration, normalized-duration, and queue-capacity limits. Defaults SHALL be 32 MiB, 10 minutes, and 8 queued requests respectively. Sample-rate conversion SHALL NOT permit input to bypass a configured limit.

#### Scenario: Encoded input is oversized
- **WHEN** encoded WAV data exceeds the configured byte limit
- **THEN** the method returns `ResourceLimit` before inference

#### Scenario: Source duration is oversized
- **WHEN** decoded source frames and the declared source rate exceed the configured duration limit
- **THEN** the method returns `ResourceLimit` before resampling or inference

#### Scenario: Normalized duration is oversized
- **WHEN** the resampled model-input frames exceed the configured duration limit at the model rate
- **THEN** the method returns `ResourceLimit` before inference

#### Scenario: Full queue rejects additional work
- **WHEN** the model worker queue is at its configured capacity
- **THEN** a new otherwise-valid request returns `ResourceLimit` rather than waiting in an unbounded queue

#### Scenario: Accepted requests are serialized
- **WHEN** multiple requests are accepted concurrently
- **THEN** they are processed by the single active model in FIFO order

### Requirement: Readiness-aware requests
Transcription SHALL only be queued while the model state is `Ready`.

#### Scenario: Request arrives during acquisition
- **WHEN** a transcription method is called while the model is initializing, downloading, or loading
- **THEN** the call returns the retryable `NotReady` D-Bus error

#### Scenario: Request arrives after model failure
- **WHEN** a transcription method is called while the model state is `Failed`
- **THEN** the call returns `ModelUnavailable`

### Requirement: Stable D-Bus errors
The interface SHALL distinguish `NotReady`, `InvalidOptions`, `InvalidAudio`, `ModelUnavailable`, `ResourceLimit`, and `TranscriptionFailed` as stable D-Bus error names under the Sophon namespace.

#### Scenario: Inference fails after acceptance
- **WHEN** the active backend fails while processing a valid queued request
- **THEN** the caller receives `TranscriptionFailed` without terminating the daemon

### Requirement: STT-only resampling boundary
Automatic WAV resampling SHALL apply only to transcription input. It SHALL NOT alter TTS file output, TTS buffer output, aloud playback samples, or TTS clone-reference input.

#### Scenario: STT input needs conversion
- **WHEN** transcription WAV audio differs from the active STT model rate
- **THEN** only the samples submitted to the STT model are resampled

#### Scenario: Non-STT audio has another rate
- **WHEN** TTS output or clone-reference audio uses a rate governed by its own contract
- **THEN** STT input normalization does not resample or relax that audio contract
