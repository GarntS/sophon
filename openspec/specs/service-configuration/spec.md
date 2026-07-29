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
When no configuration file exists, the service SHALL use documented defaults selecting a pinned Parakeet int8 model, automatic model acquisition, automatic acceleration, English transcription, translation disabled, a 32 MiB input limit, a 10-minute duration limit, and queue capacity 8.

#### Scenario: First run without configuration
- **WHEN** the discovered configuration path does not exist
- **THEN** the daemon starts model acquisition using all documented defaults

### Requirement: Supported configuration
The YAML configuration SHALL support active engine, model ID, optional model path, quantization, accelerator, default language, default translation, model cache directory, automatic-download policy, maximum audio bytes, maximum audio duration, queue capacity, and logging verbosity.

#### Scenario: Complete valid configuration is loaded
- **WHEN** a configuration file supplies valid supported fields
- **THEN** the daemon applies those values before model acquisition and serving transcription

#### Scenario: Partial valid configuration is loaded
- **WHEN** a configuration file omits optional fields
- **THEN** documented defaults fill the omitted fields

### Requirement: Strict configuration validation
A present configuration file SHALL fail validation for malformed YAML, unknown fields, inconsistent engine/model settings, unsupported quantization or accelerator values, invalid paths, and zero or out-of-range resource limits. The service SHALL NOT silently replace a present invalid configuration with defaults.

#### Scenario: Unknown configuration field is present
- **WHEN** the YAML contains a field not recognized by the running Sophon version
- **THEN** model state becomes `Failed` and `LastError` identifies the invalid field

#### Scenario: YAML is malformed
- **WHEN** the configuration file cannot be parsed
- **THEN** model state becomes `Failed` and transcription calls return `ModelUnavailable`

### Requirement: Startup-only configuration
Configuration SHALL remain immutable for the daemon process lifetime, and changing the configuration file SHALL require restarting the service.

#### Scenario: File changes while daemon is running
- **WHEN** `config.yaml` is modified after successful startup
- **THEN** active service behavior remains unchanged until the daemon restarts

#### Scenario: Daemon restarts after configuration change
- **WHEN** the user restarts the service after saving valid changed configuration
- **THEN** the new process validates and applies the changed values before loading its model

### Requirement: XDG model cache override
The default model cache SHALL be `sophon/models` beneath `$XDG_CACHE_HOME` when set and `~/.cache/sophon/models` otherwise, and a valid configured cache directory SHALL override that default.

#### Scenario: No cache override is configured
- **WHEN** model acquisition needs a cache and configuration omits `cache_dir`
- **THEN** the service uses the XDG-derived model cache path

#### Scenario: Cache override is configured
- **WHEN** configuration supplies a writable cache directory
- **THEN** automatic model acquisition and lookup use that directory

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
