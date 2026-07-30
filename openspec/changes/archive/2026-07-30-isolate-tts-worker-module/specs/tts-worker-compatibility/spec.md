## ADDED Requirements

### Requirement: Stable public TTS worker API
The system SHALL preserve the existing public Rust paths and signatures for `sophon::tts::TtsWorker` and `sophon::tts::TtsStream` when their implementation is isolated into a focused module.

#### Scenario: Existing consumer compiles after isolation
- **WHEN** a consumer imports `TtsWorker` or `TtsStream` from `sophon::tts` and calls an existing public method
- **THEN** the consumer compiles without changing its import path or method call

### Requirement: Behavior-preserving worker isolation
The isolated TTS worker SHALL retain the existing FIFO request scheduling, configured queue-capacity rejection, provider ownership, buffered synthesis, streaming synthesis, buffered-provider fallback, generated-audio validation, cancellation, terminal-event delivery, and error classifications.

#### Scenario: Worker behavior is compared before and after isolation
- **WHEN** the existing worker unit and D-Bus integration scenarios are run after the module move
- **THEN** request ordering, successful outputs, queue-full errors, provider failures, stream failures, overflow cancellation, dropped-consumer cancellation, and recovery behavior remain unchanged
