## MODIFIED Requirements

### Requirement: Verified automatic Kokoro acquisition
Sophon SHALL define Kokoro in the package-installed model registry with every required model and voice sidecar role, immutable upstream location, revision metadata, relative path, size, and SHA-256 digest. The registry SHALL automatically download missing artifacts and expose only complete verified role paths to the Kokoro provider.

#### Scenario: First TTS startup downloads Kokoro
- **WHEN** no valid Kokoro cache exists
- **THEN** the registry downloads and verifies all required artifacts before provider loading

#### Scenario: Valid cache is reused
- **WHEN** every cached Kokoro artifact matches the installed manifest
- **THEN** the provider loads it without downloading files again

#### Scenario: Download or checksum fails
- **WHEN** an artifact download is interrupted or has a mismatched digest
- **THEN** no partial model becomes loadable and TTS state remains terminal `Failed` until restart

### Requirement: Independent observable TTS lifecycle
The D-Bus interface SHALL expose read-only TTS state, active provider/model identity, download progress, last error, available voices, and capabilities sourced from the long-lived TTS provider handle and SHALL emit standard property changes. TTS states SHALL be `Initializing`, `Downloading`, `Loading`, `Ready`, and `Failed`.

#### Scenario: Client observes TTS acquisition
- **WHEN** registry resolution is downloading the selected TTS model
- **THEN** TTS state is `Downloading`, progress reports a bounded fraction, and STT state remains independent

#### Scenario: TTS becomes ready
- **WHEN** registry resolution and provider loading succeed
- **THEN** state is `Ready`, active identity names the selected pair, and voices and capabilities reflect the runtime provider

#### Scenario: TTS fails while STT is ready
- **WHEN** TTS resolution or loading fails after STT becomes ready
- **THEN** TTS state is terminal `Failed` without changing STT state

### Requirement: Curated multi-provider TTS selection
Sophon SHALL select TTS from a validated provider/model pair in the package registry while preserving Kokoro as the default. Each model SHALL identify its provider, loader kind, supported languages, revision metadata, and semantic required file roles.

#### Scenario: Existing configuration is absent
- **WHEN** the TTS section is omitted
- **THEN** Sophon selects the registered default Kokoro model

#### Scenario: Qwen model is selected
- **WHEN** configuration selects a registered `qwentts-cpp` model
- **THEN** its metadata kind selects Base, CustomVoice, or VoiceDesign loading

#### Scenario: Provider and model disagree
- **WHEN** configuration combines a provider with a model not registered beneath it
- **THEN** TTS state becomes terminal `Failed` without fallback

### Requirement: Composite TTS model resolution
A TTS model composed from multiple shared artifacts SHALL become loadable only when every registry file verifies, and resolution SHALL provide a role-keyed path mapping that the provider validates before native loading.

#### Scenario: Talker and codec are valid
- **WHEN** both selected Qwen artifact digests verify
- **THEN** resolution supplies `talker` and `codec` paths

#### Scenario: One composite artifact cannot be acquired
- **WHEN** either artifact is absent and its single download attempt fails
- **THEN** TTS state becomes terminal `Failed` without treating the partial model as ready

## REMOVED Requirements

### Requirement: Curated-only TTS model overrides
**Reason**: Local model overrides are removed; all model files are selected from the package registry and verified shared cache.

**Migration**: Package the desired curated model in `model_registry.yaml` and pre-populate verified cache blobs when necessary.
