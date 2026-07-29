## ADDED Requirements

### Requirement: Typed TTS provider configuration
Sophon SHALL validate the TTS section into a provider/model-specific configuration variant with shared operational settings. Fields not valid for the selected Kokoro, Qwen Base, Qwen CustomVoice, or Qwen VoiceDesign variant SHALL fail TTS configuration without invalidating otherwise valid STT configuration.

#### Scenario: Qwen Base configuration is partial
- **WHEN** provider `qwentts-cpp` selects a curated Base model and omits optional Qwen settings
- **THEN** Sophon constructs a Base configuration using documented common and sampling defaults without requiring a named voice or description

#### Scenario: CustomVoice default is omitted
- **WHEN** provider `qwentts-cpp` selects a CustomVoice model without `default_voice`
- **THEN** the typed variant uses documented default speaker `vivian` and validates that speaker after model loading

#### Scenario: VoiceDesign default is omitted
- **WHEN** provider `qwentts-cpp` selects a VoiceDesign model without `default_voice_description`
- **THEN** the typed variant uses `A warm, clear, natural adult voice with moderate pitch and pace.`

#### Scenario: Mode-inapplicable field is present
- **WHEN** a Base configuration supplies `default_voice` or a Kokoro configuration supplies Qwen sampling settings
- **THEN** TTS configuration fails and identifies the inapplicable field

### Requirement: Strict daemon-wide Qwen sampling configuration
A Qwen TTS variant SHALL accept an optional strict `sampling` mapping containing `seed`, `max_new_tokens`, `temperature`, `top_k`, `top_p`, and `repetition_penalty`. Values SHALL be finite where applicable and remain within documented safe ranges; unknown fields and invalid values SHALL fail TTS configuration.

#### Scenario: Sampling mapping is absent
- **WHEN** a valid Qwen variant omits `sampling`
- **THEN** it uses random seed, 2048 maximum new tokens, temperature 0.9, top-k 50, top-p 1.0, and repetition penalty 1.05

#### Scenario: Deterministic policy is configured
- **WHEN** sampling contains a valid numeric seed
- **THEN** every request in that daemon process uses that seed and the remaining configured sampling values

#### Scenario: Sampling value is invalid
- **WHEN** temperature is non-finite, a numeric range is violated, or an unknown sampling key is present
- **THEN** TTS initialization fails before model acquisition

### Requirement: Qwen mode defaults and limits
Qwen configurations SHALL require effective speed `1.0`; SHALL validate default descriptions with the configured text-byte limit and control-character policy; and SHALL preserve the existing common defaults for download, playback, reference audio, generated duration, and queue capacity.

#### Scenario: Non-unit default speed is configured
- **WHEN** a Qwen variant configures `default_speed` other than `1.0`
- **THEN** TTS configuration fails because the provider does not support speed control

#### Scenario: Oversized default description is configured
- **WHEN** a VoiceDesign default exceeds `max_text_bytes`
- **THEN** TTS configuration fails before acquisition
