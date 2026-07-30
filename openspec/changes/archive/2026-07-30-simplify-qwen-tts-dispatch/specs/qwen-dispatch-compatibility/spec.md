## ADDED Requirements

### Requirement: Stable public Qwen provider API
The system SHALL preserve the existing public Rust paths and signatures for Qwen language normalization, the Qwen engine adapter, and the Base, CustomVoice, and VoiceDesign provider types while their private dispatch is simplified.

#### Scenario: Existing Qwen consumer compiles
- **WHEN** a consumer imports and uses an existing public Qwen type or function from `sophon::tts`
- **THEN** it compiles without changing its import path, feature selection, constructor, or method call

### Requirement: Buffered and streaming mode parity
For each Qwen model mode, the simplified dispatch SHALL apply the same mode-specific default or request override, language normalization, sampling policy, capability checks, text-like input limits, speed restriction, reference validation, and native voice operation that the corresponding current buffered or streaming path applies.

#### Scenario: Every supported mode is executed both ways
- **WHEN** default, named, design, or clone intent valid for the selected model is synthesized through buffered and streamed execution
- **THEN** both paths select the same language and voice intent and differ only in buffered result delivery versus ordered stream delivery

#### Scenario: Invalid request is executed both ways
- **WHEN** the same invalid request is submitted to buffered and streaming execution for a Qwen provider
- **THEN** both paths reject it with the same Sophon error classification before native synthesis

### Requirement: Qwen mode isolation remains enforced
Simplifying dispatch SHALL NOT allow Base, CustomVoice, or VoiceDesign providers to accept another mode's voice intent or default, and SHALL NOT make a native synthesis failure terminal for the provider worker.

#### Scenario: Unsupported mode intent is supplied
- **WHEN** a Qwen provider receives a validly encoded voice intent unsupported by its model mode
- **THEN** it returns `UnsupportedCapability` without invoking a substituted native operation

#### Scenario: Native operation fails
- **WHEN** one buffered or streaming native Qwen operation fails
- **THEN** the caller receives `SynthesisFailed` and a later valid request can still use the provider
