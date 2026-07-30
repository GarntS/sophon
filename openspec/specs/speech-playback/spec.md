## ADDED Requirements

### Requirement: PipeWire speech playback
`SpeakAloud` SHALL play provider-native mono float PCM through the selected CPAL PipeWire output device without application-level output resampling and SHALL reply only after all successfully generated frames drain or the operation fails.

#### Scenario: Speech plays successfully
- **WHEN** synthesis succeeds and CPAL accepts and drains all frames at the provider's sample rate
- **THEN** `SpeakAloud` returns successfully after audible playback completes

#### Scenario: Native rate cannot be opened
- **WHEN** CPAL cannot open the selected device at the provider's sample rate
- **THEN** `SpeakAloud` returns `PlaybackFailed` without resampling the output or changing TTS model readiness

#### Scenario: Playback fails after starting
- **WHEN** the CPAL stream fails while consuming generated frames
- **THEN** `SpeakAloud` stops the operation, discards unplayed frames, and returns `PlaybackFailed` without changing TTS model readiness

### Requirement: Stable output-device selection
When a CPAL PipeWire device ID is configured, playback SHALL target that exact device; when no device is configured, playback SHALL target the CPAL PipeWire host's current default output device. Sophon SHALL NOT silently fall back if an explicitly configured device is unavailable.

#### Scenario: Configured device exists
- **WHEN** the configured `pipewire:<node.name>` device ID is available
- **THEN** synthesized speech is played only through that device

#### Scenario: No device is configured
- **WHEN** playback is requested without a configured output device
- **THEN** Sophon uses the current default output device reported by CPAL's PipeWire host

#### Scenario: Configured device is missing
- **WHEN** the configured device ID cannot be resolved for playback
- **THEN** the method returns `PlaybackFailed` and sends no speech to another device

### Requirement: Configured playback volume
Playback SHALL apply the configured finite linear volume in the inclusive range `0.0` through `1.0` to speech output, with default `1.0`.

#### Scenario: Reduced volume is configured
- **WHEN** volume is configured below `1.0`
- **THEN** every submitted speech sample is scaled according to that volume without changing file or memfd output

#### Scenario: Muted volume is configured
- **WHEN** volume is `0.0`
- **THEN** playback completes normally with silent samples

### Requirement: Non-overlapping speech playback
Accepted `SpeakAloud` operations SHALL enter a serialized FIFO stage covering streamed synthesis and playback so two calls never play concurrently.

#### Scenario: Two callers speak aloud
- **WHEN** two valid `SpeakAloud` calls are accepted concurrently
- **THEN** the second call produces no audible frames until the first playback completes or fails

### Requirement: Incremental speech playback
Playback SHALL accept bounded provider-native audio chunks and SHALL start the output stream when the first nonempty chunk is available rather than requiring the complete utterance. It SHALL write silence during temporary producer underruns and SHALL resume speech when later chunks arrive.

#### Scenario: First streaming chunk arrives
- **WHEN** a streaming provider emits its first nonempty audio chunk
- **THEN** Sophon starts playback without waiting for synthesis to complete or accumulating a startup prebuffer

#### Scenario: Synthesis temporarily underruns playback
- **WHEN** the output callback needs frames before the provider's next chunk is available
- **THEN** it writes silence for the unavailable frames and consumes later speech chunks in order when they arrive

#### Scenario: Provider does not stream
- **WHEN** the active provider supports only buffered synthesis
- **THEN** `SpeakAloud` plays its complete provider result through the same serialized playback behavior

### Requirement: Bounded real-time playback buffering
Streamed playback SHALL preserve chunk order while bounding service-owned queued handoff audio to 24,576 mono samples per accepted stream: at most 16,384 samples in the worker-to-consumer channel, at most one 4,096-sample pending playback chunk, and 4,096 frames in the output ring. A faster provider SHALL be backpressured until playback frees capacity. Blocking and allocation SHALL remain outside the real-time output callback.

#### Scenario: Provider generates faster than playback
- **WHEN** valid chunks arrive faster than the output device consumes them
- **THEN** Sophon retains at most the fixed sample budget, pauses the provider outside the real-time callback, and resumes it as playback drains samples in order

#### Scenario: Large provider chunk is emitted
- **WHEN** a provider or buffered fallback supplies more than 4,096 samples in one result event
- **THEN** Sophon hands it off as consecutive chunks of at most 4,096 samples without changing the reconstructed sample sequence

#### Scenario: Stream exceeds its duration bound
- **WHEN** accepted chunks would exceed the configured maximum generated duration
- **THEN** Sophon cancels further synthesis, discards unplayed excess data, and returns `ResourceLimit`

#### Scenario: Synthesis fails with queued audio
- **WHEN** synthesis terminates with an error while valid but unplayed chunks remain queued
- **THEN** playback observes the terminal error independently of audio-channel capacity, discards unplayed samples, and returns the original synthesis or resource error promptly
