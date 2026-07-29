## ADDED Requirements

### Requirement: Curated multi-provider TTS selection
Sophon SHALL select the active TTS implementation from a validated provider and curated model combination while preserving Kokoro as the default. A model manifest SHALL identify its provider, mode where applicable, immutable revision, and required artifact identities.

#### Scenario: Existing configuration is absent
- **WHEN** the TTS section is omitted
- **THEN** Sophon continues selecting the pinned Kokoro int8 model

#### Scenario: Qwen model is selected
- **WHEN** configuration selects provider `qwentts-cpp` and one registered Qwen model ID
- **THEN** Sophon loads the provider implementation matching the manifest's Base, CustomVoice, or VoiceDesign mode

#### Scenario: Provider and model disagree
- **WHEN** a configured provider does not own the selected curated model
- **THEN** TTS configuration fails without acquisition or fallback

### Requirement: Composite TTS model resolution
A curated TTS model composed from multiple shared artifacts SHALL become loadable only when every referenced artifact independently verifies, and resolution SHALL provide the provider with explicit paths for each required role.

#### Scenario: Talker and codec are valid
- **WHEN** both selected Qwen artifact digests verify
- **THEN** model resolution supplies their respective talker and codec paths for loading

#### Scenario: One composite artifact is absent
- **WHEN** either the selected talker or codec is unavailable and automatic download is disabled
- **THEN** TTS initialization fails with `ModelUnavailable` without treating the partial model as ready

### Requirement: Curated-only TTS model overrides
Local TTS model configuration SHALL only accept files that exactly match every artifact identity in the selected curated manifest.

#### Scenario: Self-converted GGUF is supplied
- **WHEN** a local Qwen path contains readable but non-matching GGUF data
- **THEN** initialization fails digest validation and does not substitute a registry download
