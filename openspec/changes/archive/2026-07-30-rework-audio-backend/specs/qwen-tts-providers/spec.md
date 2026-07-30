## ADDED Requirements

### Requirement: Qwen streamed aloud output
Every Sophon Qwen provider backed by qwentts.cpp SHALL advertise internal streaming synthesis support and SHALL use native Qwen audio chunks for `SpeakAloud`. Qwen file and buffer synthesis SHALL continue using the native buffered path.

#### Scenario: Qwen speaks aloud
- **WHEN** a valid Base, CustomVoice, or VoiceDesign request is submitted through `SpeakAloud`
- **THEN** the provider forwards native mono 24 kHz `f32` chunks in generation order as soon as they are available

#### Scenario: Qwen synthesizes buffered output
- **WHEN** the same provider handles `SpeakToFile` or `SpeakToBuffer`
- **THEN** it uses buffered native synthesis and returns complete mono 24 kHz audio for WAV encoding

#### Scenario: Native stream reports cancellation
- **WHEN** the playback consumer or generated-duration budget rejects further chunks
- **THEN** the provider requests native cancellation and maps the terminal cause to the corresponding Sophon error without making the provider unavailable
