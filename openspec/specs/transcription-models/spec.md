## ADDED Requirements

### Requirement: Configurable Parakeet and Canary backends
The service SHALL support selecting either a Parakeet or Canary ONNX model and SHALL load exactly one configured active model per daemon process.

#### Scenario: Parakeet is selected
- **WHEN** startup configuration selects a valid Parakeet model
- **THEN** the service loads that model and reports `parakeet` as the active engine

#### Scenario: Canary is selected
- **WHEN** startup configuration selects a valid Canary model
- **THEN** the service loads that model and reports `canary` as the active engine

#### Scenario: Unsupported engine is configured
- **WHEN** startup configuration names an engine other than the supported version-one engines
- **THEN** model state becomes `Failed` with a configuration-related error

### Requirement: Curated automatic model acquisition
When no model path override is configured, the service SHALL resolve the configured versioned model ID through a built-in registry containing its engine, HTTPS source, SHA-256 digest, supported quantization, and expected layout.

#### Scenario: Uncached model is acquired
- **WHEN** the configured registry model is absent from the model cache and downloads are permitted
- **THEN** the service downloads, verifies, validates, and atomically installs it before loading

#### Scenario: Cached model is available offline
- **WHEN** a complete verified model is present in the cache and the network is unavailable
- **THEN** the service loads the cached model without requiring a network request

#### Scenario: Model identifier is unknown
- **WHEN** configuration specifies an identifier absent from the built-in registry
- **THEN** model state becomes `Failed` and `LastError` identifies the unknown model

### Requirement: Model path override
A configured local model path SHALL take precedence over automatic lookup and download and SHALL be validated for the selected engine before loading.

#### Scenario: Valid override is configured
- **WHEN** a configured model path contains the expected files for the selected engine
- **THEN** the service loads it without accessing the model registry or network

#### Scenario: Invalid override is configured
- **WHEN** the configured path is missing, inaccessible, or has an invalid model layout
- **THEN** model state becomes `Failed` and no fallback download occurs

### Requirement: Safe cache installation
Automatic acquisition SHALL use a cross-process lock, temporary storage, SHA-256 verification, expected-layout validation, and atomic publication. Partial or invalid artifacts SHALL NOT be treated as cached models.

#### Scenario: Digest verification fails
- **WHEN** downloaded bytes do not match the registry SHA-256 digest
- **THEN** the artifact is rejected, model state becomes `Failed`, and no completed cache directory is published

#### Scenario: Download is interrupted
- **WHEN** acquisition terminates before successful verification and publication
- **THEN** a later daemon run does not treat the partial artifact as a valid model

#### Scenario: Two daemon processes acquire the same model
- **WHEN** acquisition for one model ID is attempted concurrently
- **THEN** locking prevents both processes from publishing conflicting cache contents

### Requirement: Observable model lifecycle
The D-Bus interface SHALL expose read-only `State`, `ActiveEngine`, `ActiveModel`, `DownloadProgress`, and `LastError` properties and SHALL emit property-change notifications when their observable values change.

#### Scenario: First-use download progresses
- **WHEN** the daemon acquires an uncached model
- **THEN** `State` transitions through `Initializing`, `Downloading`, `Loading`, and `Ready`, with download progress updated between zero and one

#### Scenario: Model initialization fails
- **WHEN** acquisition, validation, or loading fails
- **THEN** `State` becomes `Failed`, `LastError` contains an actionable description, and a property-change notification is emitted

### Requirement: Hardware accelerator policy
The service SHALL support `auto`, `cpu`, `cuda`, and `rocm` accelerator configuration subject to providers compiled into the installed package. `auto` SHALL allow CPU fallback; explicitly requested unavailable providers SHALL fail model initialization.

#### Scenario: Auto acceleration has no usable GPU provider
- **WHEN** acceleration is `auto` and no compiled GPU provider can initialize
- **THEN** the service loads the active model with the CPU provider

#### Scenario: Explicit provider is unavailable
- **WHEN** configuration explicitly requests CUDA or ROCm but that provider is not compiled or cannot initialize
- **THEN** model state becomes `Failed` rather than silently using CPU

#### Scenario: Explicit available provider initializes
- **WHEN** configuration requests a compiled and operational GPU provider
- **THEN** the active model uses that provider for ONNX inference
