## ADDED Requirements

### Requirement: Independent TTS configuration
The startup YAML configuration SHALL accept an optional strict TTS section containing provider and model identifiers, optional absolute local model path, optional cache override, automatic-download policy, default voice, default speed, optional PipeWire node name, playback volume, maximum text bytes, reference-audio bytes and duration, generated-output duration, and queue capacity.

#### Scenario: Partial TTS configuration is loaded
- **WHEN** the TTS section supplies only some supported fields
- **THEN** documented TTS defaults fill omitted fields without changing STT configuration

#### Scenario: Complete TTS configuration is loaded
- **WHEN** all TTS fields contain valid supported values
- **THEN** Sophon applies them before TTS acquisition, provider loading, request acceptance, or playback

### Requirement: Documented TTS defaults
When TTS configuration is omitted, Sophon SHALL select the pinned Kokoro int8 `tts-rs` provider model, automatic verified acquisition, voice `af_heart`, speed `1.0`, PipeWire's default sink, volume `1.0`, 16 KiB maximum text, 32 MiB and 60 seconds maximum reference audio, 600 seconds maximum generated output, and queue capacity 8.

#### Scenario: First startup without a TTS section
- **WHEN** a valid configuration omits the TTS section
- **THEN** independent TTS initialization begins using every documented TTS default

### Requirement: Strict TTS configuration validation
A present TTS section SHALL fail TTS initialization for unknown fields, malformed values, unknown provider or model combinations, invalid model or cache paths, empty default voice, non-finite or out-of-range speed or volume, empty configured node name, and zero or out-of-range resource limits. Invalid TTS configuration SHALL NOT silently use defaults and SHALL NOT invalidate otherwise valid STT configuration.

#### Scenario: Unknown TTS field is present
- **WHEN** the TTS mapping contains an unrecognized field
- **THEN** `TtsState` becomes `Failed`, `TtsLastError` identifies the invalid field, and STT initialization proceeds independently

#### Scenario: Playback volume is invalid
- **WHEN** configured TTS volume is non-finite or outside inclusive range `0.0` through `1.0`
- **THEN** TTS configuration fails before model acquisition

#### Scenario: Resource limit is zero
- **WHEN** any configured TTS text, audio, duration, or queue limit is zero
- **THEN** TTS configuration fails rather than creating an unbounded or unusable service

### Requirement: Startup-only TTS configuration
TTS provider, model, defaults, limits, and playback settings SHALL remain immutable for the daemon process lifetime, and file changes SHALL require a daemon restart.

#### Scenario: TTS configuration changes while running
- **WHEN** the configuration file is modified after TTS initialization
- **THEN** active synthesis and playback behavior remains unchanged until restart

### Requirement: XDG TTS model cache
The default TTS model cache SHALL be an independent entry beneath Sophon's XDG model cache, and a valid configured TTS cache directory SHALL override it.

#### Scenario: TTS cache override is omitted
- **WHEN** automatic Kokoro acquisition needs storage and no TTS cache override is configured
- **THEN** Sophon stores the pinned TTS model beneath the XDG-derived Sophon model cache

#### Scenario: TTS cache override is configured
- **WHEN** a valid TTS cache directory is supplied
- **THEN** TTS acquisition and validation use that directory without changing the STT cache location
