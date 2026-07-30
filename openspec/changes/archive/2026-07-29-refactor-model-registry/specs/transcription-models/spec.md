## MODIFIED Requirements

### Requirement: Configurable Parakeet and Canary backends
The service SHALL support selecting a registered STT provider/model pair whose package metadata kind is `parakeet` or `canary`, and SHALL load exactly one configured active STT model per daemon process.

#### Scenario: Parakeet model is selected
- **WHEN** startup configuration selects a registered model with kind `parakeet`
- **THEN** the STT provider loads it and reports the selected model as active

#### Scenario: Canary model is selected
- **WHEN** startup configuration selects a registered model with kind `canary`
- **THEN** the STT provider loads it and reports the selected model as active

#### Scenario: Unsupported kind is installed
- **WHEN** selected model metadata names an unsupported STT kind
- **THEN** STT state becomes terminal `Failed` with a registry-related error

### Requirement: Curated automatic model acquisition
The service SHALL resolve the configured STT provider/model pair through the package-installed registry and SHALL automatically download, verify, and assemble every missing required file before loading.

#### Scenario: Uncached model is resolved
- **WHEN** the configured registry model is absent from the model cache
- **THEN** the registry downloads and verifies it before the provider loads

#### Scenario: Cached model is available offline
- **WHEN** a complete verified model is present and the network is unavailable
- **THEN** the provider loads it without a network request

#### Scenario: Model identifier is unknown
- **WHEN** configuration specifies a provider/model pair absent from the package registry
- **THEN** STT state becomes terminal `Failed` and the error identifies the unknown pair

### Requirement: Observable model lifecycle
The D-Bus interface SHALL expose read-only `State`, active provider/model identity, `DownloadProgress`, and `LastError` properties sourced from the STT provider handle and SHALL emit property-change notifications when observable values change.

#### Scenario: First-use download progresses
- **WHEN** the registry acquires the selected uncached model
- **THEN** state transitions through `Initializing`, `Downloading`, `Loading`, and `Ready`, with bounded download progress

#### Scenario: Model initialization fails
- **WHEN** registry resolution or provider loading fails
- **THEN** state becomes terminal `Failed`, `LastError` contains an actionable description, and a property-change notification is emitted

## REMOVED Requirements

### Requirement: Model path override
**Reason**: Model files are exclusively defined by the package registry and verified shared cache.

**Migration**: Add the desired curated model to the package registry and pre-populate its verified cache when offline operation is required.
