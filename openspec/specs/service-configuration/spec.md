## ADDED Requirements

### Requirement: XDG configuration discovery
At startup, the service SHALL read `sophon/config.yaml` beneath `$XDG_CONFIG_HOME` when set and SHALL otherwise use `~/.config/sophon/config.yaml`.

#### Scenario: XDG configuration home is set
- **WHEN** `XDG_CONFIG_HOME` is set when the daemon starts
- **THEN** the service looks for configuration only at `$XDG_CONFIG_HOME/sophon/config.yaml`

#### Scenario: XDG configuration home is unset
- **WHEN** `XDG_CONFIG_HOME` is unset when the daemon starts
- **THEN** the service looks for configuration at `~/.config/sophon/config.yaml`

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

### Requirement: Startup-only configuration
Configuration SHALL remain immutable for the daemon process lifetime, and changing the configuration file SHALL require restarting the service.

#### Scenario: File changes while daemon is running
- **WHEN** `config.yaml` is modified after successful startup
- **THEN** active service behavior remains unchanged until the daemon restarts

#### Scenario: Daemon restarts after configuration change
- **WHEN** the user restarts the service after saving valid changed configuration
- **THEN** the new process validates and applies the changed values before loading its model

### Requirement: XDG model cache override
The default model cache SHALL be `sophon/models` beneath `$XDG_CACHE_HOME` when set and `~/.cache/sophon/models` otherwise. A configured cache directory SHALL be absolute, SHALL be either nonexistent or an existing directory, and SHALL override the default as the one shared root for registry artifact acquisition, verified model views, and provider cache data. An invalid configured root SHALL fail strict configuration before model resolution and SHALL NOT silently select the XDG default.

#### Scenario: No cache override is configured
- **WHEN** model acquisition needs a cache and configuration omits `cache_dir`
- **THEN** the service uses the XDG-derived model cache path as the shared registry root

#### Scenario: Cache override is configured
- **WHEN** configuration supplies a valid absolute cache directory
- **THEN** automatic model acquisition, lookup, assembled views, and provider cache data use that directory

#### Scenario: Nonexistent absolute cache override is configured
- **WHEN** configuration supplies an absolute cache path that does not yet exist
- **THEN** configuration accepts it and model acquisition creates the required cache directories on first use

#### Scenario: Invalid cache override is configured
- **WHEN** configuration supplies a relative cache path or a path that exists as a non-directory
- **THEN** strict configuration fails before registry resolution without falling back to the XDG cache

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

### Requirement: Startup-only TTS configuration
TTS provider, model, defaults, limits, and playback settings SHALL remain immutable for the daemon process lifetime, and file changes SHALL require a daemon restart.

#### Scenario: TTS configuration changes while running
- **WHEN** the configuration file is modified after TTS initialization
- **THEN** active synthesis and playback behavior remains unchanged until restart

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
