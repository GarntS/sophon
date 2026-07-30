## Requirements

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

### Requirement: Conservative Qwen language handling
An omitted Qwen language SHALL select native automatic detection. Sophon SHALL case-insensitively normalize documented base and regional tags for English, Chinese, Japanese, Korean, German, French, Russian, Portuguese, Spanish, and Italian, and SHALL reject unknown tags without falling back to English.

#### Scenario: Language is omitted
- **WHEN** a valid Qwen request has no language option
- **THEN** synthesis uses native automatic language detection

#### Scenario: Supported regional language is supplied
- **WHEN** a request supplies a recognized regional tag such as `en-US`, `zh-CN`, or `pt-BR`
- **THEN** Sophon selects the corresponding supported Qwen language

#### Scenario: Unsupported language is supplied
- **WHEN** a request supplies a language tag with no documented Qwen mapping
- **THEN** Sophon returns `InvalidTtsOptions` before queueing inference

### Requirement: Daemon-wide Qwen sampling
Qwen synthesis SHALL use one startup-validated sampling policy for every request. Omitted settings SHALL default to a random seed, 2048 maximum new tokens, temperature 0.9, top-k 50, top-p 1.0, and repetition penalty 1.05, and request options SHALL NOT override that policy.

#### Scenario: Sampling is omitted
- **WHEN** a Qwen configuration contains no sampling mapping
- **THEN** every synthesis uses the documented sane defaults with a newly resolved random seed

#### Scenario: Sampling is configured
- **WHEN** valid sampling values including an optional seed are configured
- **THEN** every Qwen request uses that immutable daemon-wide policy

#### Scenario: Generated duration is lower than sampling maximum
- **WHEN** the configured generated-duration limit converts to fewer tokens than `max_new_tokens`
- **THEN** Qwen generation uses the lower native duration-derived token cap and the completed audio remains subject to the final duration check

### Requirement: Qwen native diagnostics and logging
Sophon SHALL convert Qwen load and synthesis failures into its stable TTS errors and SHALL route qwentts.cpp debug, informational, warning, and error log events into corresponding `tracing` events without retaining borrowed native message storage.

#### Scenario: Native synthesis fails
- **WHEN** qwentts.cpp rejects a mode or fails generation
- **THEN** the caller receives a safe `SynthesisFailed` diagnostic and the worker remains available for subsequent requests

#### Scenario: Native worker thread logs
- **WHEN** qwentts.cpp emits a log event from a caller or internal worker thread
- **THEN** Sophon records a corresponding reentrant `tracing` event at the mapped level
