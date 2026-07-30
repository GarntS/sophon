## MODIFIED Requirements

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
