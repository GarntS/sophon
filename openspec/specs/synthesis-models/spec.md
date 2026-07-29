## ADDED Requirements

### Requirement: Provider-neutral TTS behavior
Sophon SHALL represent synthesis requests, provider capabilities, voice names, and owned PCM results independently of a specific TTS engine. Providers SHALL return owned mono `f32` samples with a sample rate and SHALL distinguish default, named, one-shot clone, and voice-design intents.

#### Scenario: Provider result is consumed by any output
- **WHEN** a provider returns successful owned PCM
- **THEN** the same result can be encoded to a file or memfd or submitted to playback without provider-specific output behavior

#### Scenario: Alternate provider is added
- **WHEN** an engine supports the provider-neutral requests and owned result contract
- **THEN** it can be integrated without changing the three D-Bus synthesis method signatures

### Requirement: Initial Kokoro provider
The initial active implementation SHALL use the Kokoro engine from `tts-rs`, SHALL support its named voices and speed control, and SHALL accurately report that one-shot cloning and voice design are unsupported.

#### Scenario: Kokoro named voice synthesis succeeds
- **WHEN** a caller selects a voice exposed by the loaded Kokoro model with valid text and speed
- **THEN** Sophon submits that selection to `tts-rs` and returns its mono 24 kHz `f32` result or a concrete synthesis failure

#### Scenario: Kokoro cloning is rejected
- **WHEN** a caller requests one-shot cloning from the Kokoro provider
- **THEN** Sophon returns `UnsupportedCapability` before invoking Kokoro inference

### Requirement: Verified automatic Kokoro acquisition
Sophon SHALL define a pinned Kokoro int8 model manifest containing every required model and voice artifact, immutable upstream locations, revisions, relative paths, and SHA-256 digests. With automatic download enabled, it SHALL download missing artifacts, verify every digest, and atomically publish only a complete valid cache entry.

#### Scenario: First TTS startup downloads Kokoro
- **WHEN** no valid Kokoro cache exists and automatic download is enabled
- **THEN** Sophon downloads and verifies all required artifacts before loading the provider

#### Scenario: Valid cache is reused
- **WHEN** every cached Kokoro artifact matches the pinned manifest
- **THEN** Sophon loads it without downloading the files again

#### Scenario: Download or checksum fails
- **WHEN** an artifact download is interrupted or has a mismatched SHA-256 digest
- **THEN** no partial model becomes loadable and TTS lifecycle becomes `Failed` with a diagnostic

#### Scenario: Local override is invalid
- **WHEN** a configured local TTS model path lacks or mismatches a required artifact
- **THEN** TTS initialization fails and Sophon does not silently download a replacement

### Requirement: Bounded serialized TTS inference
The active mutable TTS provider SHALL process accepted synthesis requests in FIFO order through one bounded queue and SHALL continue serving later requests after an individual synthesis failure.

#### Scenario: Concurrent requests are accepted
- **WHEN** multiple synthesis requests fit within queue capacity
- **THEN** provider inference processes them serially in acceptance order

#### Scenario: One request fails
- **WHEN** provider inference fails for one accepted request
- **THEN** that caller receives `SynthesisFailed` and the worker processes the next queued request

### Requirement: Independent observable TTS lifecycle
The D-Bus interface SHALL expose read-only `TtsState`, `ActiveTtsProvider`, `ActiveTtsModel`, `TtsDownloadProgress`, `TtsLastError`, `AvailableVoices`, and `TtsCapabilities` properties and SHALL emit standard `PropertiesChanged` notifications when they change. TTS states SHALL be `Initializing`, `Downloading`, `Loading`, `Ready`, and `Failed`.

#### Scenario: Client observes TTS acquisition
- **WHEN** the TTS model is downloading
- **THEN** `TtsState` is `Downloading`, progress reports a bounded fraction, and STT lifecycle properties retain their independent values

#### Scenario: TTS becomes ready
- **WHEN** the provider and model load successfully
- **THEN** active provider and model identify Kokoro, available voices are owned names from the model, capabilities reflect supported operations, and `TtsState` is `Ready`

#### Scenario: TTS fails while STT is ready
- **WHEN** TTS initialization fails after STT becomes ready
- **THEN** `TtsState` and `TtsLastError` report the failure without changing STT `State` or disabling transcription

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
