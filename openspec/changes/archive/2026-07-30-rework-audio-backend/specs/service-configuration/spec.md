## MODIFIED Requirements

### Requirement: Independent TTS configuration
The startup YAML SHALL accept an optional strict TTS section containing provider/model identifiers, mode-applicable defaults, Qwen sampling, default speed, optional CPAL PipeWire output device ID, playback volume, text and audio limits, generated duration, and queue capacity. It SHALL NOT accept a legacy PipeWire node field, model path, cache override, or automatic-download field.

#### Scenario: Partial TTS configuration is loaded
- **WHEN** the TTS section supplies only supported fields applicable to its selected model kind
- **THEN** documented defaults fill omitted values without changing STT configuration

#### Scenario: Explicit output device is configured
- **WHEN** `tts.output_device` contains a valid canonical `pipewire:<node.name>` CPAL device ID
- **THEN** the immutable playback configuration retains that exact device selection

#### Scenario: Removed TTS field is present
- **WHEN** TTS configuration supplies `pipewire_node`, a local model path, cache override, or download policy
- **THEN** TTS configuration fails strictly without invalidating STT configuration

### Requirement: Documented TTS defaults
When TTS configuration is omitted, Sophon SHALL select the pinned Kokoro int8 `tts-rs` provider model, automatic verified acquisition, voice `af_heart`, speed `1.0`, CPAL's current default PipeWire output device, volume `1.0`, 16 KiB maximum text, 32 MiB and 60 seconds maximum reference audio, 600 seconds maximum generated output, and queue capacity 8.

#### Scenario: First startup without a TTS section
- **WHEN** a valid configuration omits the TTS section
- **THEN** independent TTS initialization begins using every documented TTS default

### Requirement: Strict TTS configuration validation
A present TTS section SHALL fail TTS initialization for unknown fields, malformed values, unknown provider or model combinations, invalid model or cache paths, empty default voice, non-finite or out-of-range speed or volume, an empty, malformed, or non-PipeWire output device ID, and zero or out-of-range resource limits. Invalid TTS configuration SHALL NOT silently use defaults and SHALL NOT invalidate otherwise valid STT configuration.

#### Scenario: Unknown TTS field is present
- **WHEN** the TTS mapping contains an unrecognized field
- **THEN** `TtsState` becomes `Failed`, `TtsLastError` identifies the invalid field, and STT initialization proceeds independently

#### Scenario: Output device ID is invalid
- **WHEN** `tts.output_device` is empty, malformed, or identifies a CPAL host other than PipeWire
- **THEN** TTS configuration fails before model acquisition

#### Scenario: Playback volume is invalid
- **WHEN** configured TTS volume is non-finite or outside inclusive range `0.0` through `1.0`
- **THEN** TTS configuration fails before model acquisition

#### Scenario: Resource limit is zero
- **WHEN** any configured TTS text, audio, duration, or queue limit is zero
- **THEN** TTS configuration fails rather than creating an unbounded or unusable service

## REMOVED Requirements

### Requirement: PipeWire node configuration
**Reason**: Direct PipeWire node resolution is replaced by CPAL's canonical PipeWire device identifiers.

**Migration**: Replace `tts.pipewire_node: <node.name>` with `tts.output_device: pipewire:<node.name>`.
