## ADDED Requirements

### Requirement: Session D-Bus transcription interface
The system SHALL own `com.garntresearch.sophon` on the user session bus and SHALL export interface `com.garntresearch.sophon` at `/com/garntresearch/sophon` with `TranscribeFile` and `TranscribeMemfd` methods.

#### Scenario: Introspection exposes the transcription methods
- **WHEN** a client introspects `/com/garntresearch/sophon`
- **THEN** the `com.garntresearch.sophon` interface lists `TranscribeFile` and `TranscribeMemfd` with audio input, options dictionary, and text output arguments

### Requirement: File transcription
The `TranscribeFile` method SHALL accept an absolute path to a regular WAV file and an options dictionary, SHALL perform one transcription operation, and SHALL return the final transcribed text in the method reply.

#### Scenario: Valid file is transcribed
- **WHEN** a client calls `TranscribeFile` with an accessible absolute path containing supported WAV audio and valid options while the model is ready
- **THEN** the method returns the transcription as a string

#### Scenario: Relative file path is rejected
- **WHEN** a client calls `TranscribeFile` with a relative path
- **THEN** the method returns an `InvalidAudio` error without queueing inference

#### Scenario: Non-regular file is rejected
- **WHEN** the path identifies an object other than a regular file
- **THEN** the method returns an `InvalidAudio` error

### Requirement: Unix-FD transcription
The `TranscribeMemfd` method SHALL accept a transferred readable, seekable Unix file descriptor containing a complete WAV file and an options dictionary, SHALL read from offset zero, and SHALL return the final transcribed text. The descriptor SHALL NOT be required to identify a literal Linux memfd.

#### Scenario: Valid memfd is transcribed
- **WHEN** a client transfers a seekable descriptor containing supported WAV audio while the model is ready
- **THEN** the service reads the WAV from offset zero and returns the transcription as a string

#### Scenario: Non-seekable descriptor is rejected
- **WHEN** a client transfers a descriptor that cannot seek to offset zero
- **THEN** the method returns an `InvalidAudio` error without queueing inference

### Requirement: Canonical WAV input
The service SHALL accept only complete RIFF/WAVE audio containing one channel, a 16,000 Hz sample rate, signed 16-bit integer PCM samples, and SHALL reject malformed or unsupported audio.

#### Scenario: Canonical WAV is accepted
- **WHEN** input is a valid mono 16 kHz signed 16-bit PCM WAV within resource limits
- **THEN** the input is decoded and submitted for transcription

#### Scenario: Unsupported encoding is rejected
- **WHEN** input uses another sample rate, channel count, bit depth, sample encoding, or container
- **THEN** the method returns an `InvalidAudio` error describing the incompatible property

### Requirement: Per-request transcription options
Both transcription methods SHALL accept an `a{sv}` options dictionary recognizing `language` as a string and `translate` as a boolean. Omitted values SHALL use configured defaults, and `translate=true` SHALL mean translation to English.

#### Scenario: Request language overrides the default
- **WHEN** a client supplies a supported `language` value
- **THEN** that language is used for the request without reloading the active model

#### Scenario: Translation is requested on a capable model
- **WHEN** a client supplies `translate=true` and the active model supports translation
- **THEN** the returned text is translated to English without reloading the model

#### Scenario: Invalid option is rejected
- **WHEN** options contain an unknown key, a value of the wrong type, an unsupported language, or translation unsupported by the active model
- **THEN** the method returns an `InvalidOptions` error without queueing inference

### Requirement: Bounded short-recording workload
The service SHALL enforce configurable encoded-size, decoded-duration, and queue-capacity limits. Defaults SHALL be 32 MiB, 10 minutes, and 8 queued requests respectively.

#### Scenario: Oversized audio is rejected
- **WHEN** input exceeds either configured audio limit
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
