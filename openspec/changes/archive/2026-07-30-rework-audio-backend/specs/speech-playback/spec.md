## MODIFIED Requirements

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

### Requirement: Non-overlapping speech playback
Accepted `SpeakAloud` operations SHALL enter a serialized FIFO stage covering streamed synthesis and playback so two calls never play concurrently.

#### Scenario: Two callers speak aloud
- **WHEN** two valid `SpeakAloud` calls are accepted concurrently
- **THEN** the second call produces no audible frames until the first playback completes or fails

## ADDED Requirements

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
Streamed playback SHALL bound queued audio by the configured maximum generated duration, SHALL preserve chunk order, and SHALL keep blocking or allocation out of the real-time output callback.

#### Scenario: Provider generates faster than playback
- **WHEN** valid chunks arrive faster than the output device consumes them
- **THEN** Sophon queues them in order up to the configured generated-audio bound without blocking the provider callback

#### Scenario: Stream exceeds its bound
- **WHEN** accepted chunks would exceed the configured maximum generated duration
- **THEN** Sophon cancels further synthesis, discards unplayed excess data, and returns `ResourceLimit`
