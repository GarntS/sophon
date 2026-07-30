## MODIFIED Requirements

### Requirement: Documented defaults
When no configuration file exists, the service SHALL use documented defaults selecting a registered STT provider/model pair, automatic acceleration, English transcription, a 32 MiB input limit, a 10-minute duration limit, and queue capacity 8.

#### Scenario: First run without configuration
- **WHEN** the discovered configuration path does not exist
- **THEN** the daemon initializes the default provider handle and registry model using all documented defaults

### Requirement: Supported configuration
The YAML configuration SHALL support STT provider and model IDs, accelerator, default language, shared model cache directory, maximum audio bytes, maximum audio duration, queue capacity, and logging verbosity. It SHALL NOT support engine, quantization, translation, local model path, or automatic-download policy fields.

#### Scenario: Complete valid configuration is loaded
- **WHEN** a configuration file supplies valid supported fields
- **THEN** the daemon applies them before registry resolution and provider loading

#### Scenario: Removed acquisition field is present
- **WHEN** configuration supplies a local model path or automatic-download policy
- **THEN** strict validation fails rather than changing registry behavior

### Requirement: Strict configuration validation
A present configuration file SHALL fail validation for malformed YAML, unknown fields, unknown provider/model pairs, unsupported accelerator values, invalid cache paths, and zero or out-of-range resource limits. The service SHALL NOT silently replace invalid configuration with defaults.

#### Scenario: Unknown provider/model pair is configured
- **WHEN** configuration names a pair absent from the package registry
- **THEN** STT state becomes terminal `Failed` and the error identifies the pair

#### Scenario: YAML is malformed
- **WHEN** the configuration file cannot be parsed
- **THEN** STT state becomes terminal `Failed` and transcription calls return `ModelUnavailable`

### Requirement: Independent TTS configuration
The startup YAML SHALL accept an optional strict TTS section containing provider/model identifiers, mode-applicable defaults, Qwen sampling, default speed, optional PipeWire node, playback volume, text and audio limits, generated duration, and queue capacity. It SHALL NOT accept model-path, cache override, or automatic-download fields.

#### Scenario: Partial TTS configuration is loaded
- **WHEN** the TTS section supplies only supported fields applicable to its selected model kind
- **THEN** documented defaults fill omitted values without changing STT configuration

#### Scenario: Removed TTS acquisition field is present
- **WHEN** TTS configuration supplies a local model path, cache override, or download policy
- **THEN** TTS configuration fails strictly without invalidating STT configuration

### Requirement: Typed TTS provider configuration
Sophon SHALL validate TTS configuration against the selected registry model kind. Base SHALL accept default clone reference path and optional transcript, CustomVoice SHALL accept a default named voice, and VoiceDesign SHALL accept a default prompt; inapplicable fields SHALL fail TTS configuration.

#### Scenario: Base defaults are omitted
- **WHEN** a Base model omits clone defaults
- **THEN** the provider uses its documented sane default behavior

#### Scenario: CustomVoice default is omitted
- **WHEN** a CustomVoice model omits `default_voice`
- **THEN** the provider uses documented default speaker `vivian` and validates it after loading

#### Scenario: VoiceDesign default is omitted
- **WHEN** a VoiceDesign model omits its default prompt
- **THEN** the provider uses `A warm, clear, natural adult voice with moderate pitch and pace.`

#### Scenario: Mode-inapplicable field is present
- **WHEN** configuration supplies a default that is not valid for the selected registry kind
- **THEN** TTS configuration fails and identifies the inapplicable field

### Requirement: Request precedence over TTS defaults
Valid request-level clone audio and transcript, named voice, or voice-design prompt SHALL override the corresponding startup default for that synthesis only and SHALL NOT mutate daemon configuration.

#### Scenario: Request overrides default voice
- **WHEN** a CustomVoice request selects another available named voice
- **THEN** that request uses the selected voice and later default requests use the configured voice

#### Scenario: Request overrides default clone reference
- **WHEN** a Base request supplies valid clone audio and optional transcript
- **THEN** that request uses the supplied reference and later default requests retain configured Base defaults

#### Scenario: Request overrides default prompt
- **WHEN** a VoiceDesign request supplies a valid description
- **THEN** that request uses the description and later default requests retain the configured prompt

## REMOVED Requirements

### Requirement: XDG TTS model cache
**Reason**: One singleton registry manages a shared model cache for STT and TTS.

**Migration**: Use the shared Sophon model cache setting; provider-specific cache overrides are no longer accepted.
