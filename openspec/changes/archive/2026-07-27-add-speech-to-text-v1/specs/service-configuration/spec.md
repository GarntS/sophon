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
