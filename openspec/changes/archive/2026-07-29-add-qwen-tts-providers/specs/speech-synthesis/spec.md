## ADDED Requirements

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
