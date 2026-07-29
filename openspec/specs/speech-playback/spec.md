## ADDED Requirements

### Requirement: PipeWire speech playback
`SpeakAloud` SHALL synthesize a valid request, play the complete provider-native float PCM through PipeWire, and reply only after playback drains successfully or fails.

#### Scenario: Speech plays successfully
- **WHEN** synthesis succeeds and PipeWire accepts and drains all frames
- **THEN** `SpeakAloud` returns successfully after audible playback completes

#### Scenario: Playback fails
- **WHEN** PipeWire cannot connect, configure a compatible stream, submit frames, or drain playback
- **THEN** `SpeakAloud` returns `PlaybackFailed` without changing TTS model readiness

### Requirement: Stable output-device selection
When a PipeWire node name is configured, playback SHALL target that exact stable `node.name`; when no node is configured, playback SHALL target PipeWire's current default audio sink. Sophon SHALL NOT silently fall back if an explicitly configured node is unavailable.

#### Scenario: Configured node exists
- **WHEN** the configured PipeWire node name is available
- **THEN** synthesized speech is played only through that node

#### Scenario: No node is configured
- **WHEN** playback is requested without a configured node name
- **THEN** Sophon uses PipeWire's current default sink

#### Scenario: Configured node is missing
- **WHEN** the configured node name cannot be resolved for playback
- **THEN** the method returns `PlaybackFailed` and sends no speech to another sink

### Requirement: Configured playback volume
Playback SHALL apply the configured finite linear volume in the inclusive range `0.0` through `1.0` to speech output, with default `1.0`.

#### Scenario: Reduced volume is configured
- **WHEN** volume is configured below `1.0`
- **THEN** every submitted speech sample is scaled according to that volume without changing file or memfd output

#### Scenario: Muted volume is configured
- **WHEN** volume is `0.0`
- **THEN** playback completes normally with silent samples

### Requirement: Non-overlapping speech playback
Accepted `SpeakAloud` operations SHALL enter a serialized FIFO playback stage so two calls never play concurrently.

#### Scenario: Two callers speak aloud
- **WHEN** two valid `SpeakAloud` calls are accepted concurrently
- **THEN** the second playback begins only after the first playback completes or fails
