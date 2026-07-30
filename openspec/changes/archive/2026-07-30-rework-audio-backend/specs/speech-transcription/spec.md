## MODIFIED Requirements

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

## ADDED Requirements

### Requirement: STT-only resampling boundary
Automatic WAV resampling SHALL apply only to transcription input. It SHALL NOT alter TTS file output, TTS buffer output, aloud playback samples, or TTS clone-reference input.

#### Scenario: STT input needs conversion
- **WHEN** transcription WAV audio differs from the active STT model rate
- **THEN** only the samples submitted to the STT model are resampled

#### Scenario: Non-STT audio has another rate
- **WHEN** TTS output or clone-reference audio uses a rate governed by its own contract
- **THEN** STT input normalization does not resample or relax that audio contract
