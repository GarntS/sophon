## MODIFIED Requirements

### Requirement: Mode-specific Qwen providers
Sophon SHALL provide Base, CustomVoice, and VoiceDesign models under provider ID `qwentts-cpp`; each registered model kind SHALL advertise and accept only supported voice intents. Startup-validated configuration SHALL provide mode-specific defaults, and a valid request-level intent SHALL override the corresponding default for that request only.

#### Scenario: Base model is ready
- **WHEN** a registered Base model loads successfully
- **THEN** Sophon advertises one-shot voice cloning and accepts default or clone intents, using configured default clone reference data when no request clone is supplied

#### Scenario: CustomVoice model is ready
- **WHEN** a registered CustomVoice model loads successfully
- **THEN** Sophon advertises named voices and uses the configured default speaker when no request voice is supplied

#### Scenario: VoiceDesign model is ready
- **WHEN** a registered VoiceDesign model loads successfully
- **THEN** Sophon advertises voice design and uses the configured default prompt when no request description is supplied

#### Scenario: Request intent overrides configured default
- **WHEN** a request supplies a valid clone reference, named voice, or voice description supported by the selected model kind
- **THEN** Sophon uses that request value for only that synthesis

### Requirement: Package-defined Q8_0 Qwen catalog
The package registry SHALL define the 0.6B and 1.7B Base, 0.6B and 1.7B CustomVoice, and 1.7B VoiceDesign Q8_0 models under `qwentts-cpp`, with semantic `talker` and shared `codec` roles from immutable revision `e0f336a048a3de02b29b8ad92969217d9ecffe3e`; every artifact SHALL retain its exact expected size and SHA-256 digest.

#### Scenario: Any packaged model is selected
- **WHEN** configuration selects one of the five registered Qwen model IDs
- **THEN** the registry resolves its talker and shared codec and verifies both before loading

#### Scenario: Unregistered Qwen model is selected
- **WHEN** configuration names another model ID
- **THEN** TTS state becomes terminal `Failed` without loading or downloading an alternative
