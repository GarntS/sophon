## ADDED Requirements

### Requirement: Provider-owned model state
Each configured STT and TTS provider SHALL expose model state through a long-lived handle that exists before asynchronous initialization. Model state SHALL be a data-only enum with `Initializing`, `Downloading`, `Loading`, `Ready`, and `Failed` variants.

#### Scenario: Initialization has not started
- **WHEN** the provider handle exists but initialization has not begun
- **THEN** it reports `Initializing`

#### Scenario: Provider is inference-ready
- **WHEN** native loading succeeds and the provider worker can accept work
- **THEN** the handle reports `Ready`

### Requirement: Registry-backed acquisition state
While a provider waits for artifacts, its state accessor SHALL derive `Downloading` progress or terminal `Failed` state from the singleton registry. After artifacts resolve, the provider SHALL report its own `Loading`, `Ready`, or terminal `Failed` runtime state.

#### Scenario: Artifact download is in progress
- **WHEN** registry resolution is downloading bytes for the provider's model
- **THEN** the provider reports `Downloading` with bounded byte-weighted progress

#### Scenario: Native loader is constructing the model
- **WHEN** all artifacts are available but the runtime worker is not yet usable
- **THEN** the provider reports `Loading`

#### Scenario: Registry acquisition fails
- **WHEN** the registry records terminal failure for the selected model
- **THEN** the provider reports `Failed` with that diagnostic for the remainder of the daemon process

### Requirement: Independent provider state
STT and TTS provider handles SHALL retain independent state, errors, active model identity, and discovered runtime capabilities, and D-Bus lifecycle properties SHALL read from the corresponding handle.

#### Scenario: TTS fails while STT is ready
- **WHEN** TTS acquisition or loading fails after STT becomes ready
- **THEN** TTS reports `Failed` without changing STT state or disabling transcription
