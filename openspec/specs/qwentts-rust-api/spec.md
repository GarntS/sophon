## Requirements

### Requirement: Stateful Qwen TTS engine lifecycle
The crate SHALL expose a safe, stateful Qwen TTS engine API with construction, paired talker-and-codec model loading, unloading, and automatic native-resource release. The engine API SHALL follow the usage shape of `tts-rs`: callers create an engine, load a model before synthesis, and pass optional request options to synthesis. Calling synthesis while no model is loaded SHALL return a crate-defined error rather than invoke native synthesis.

#### Scenario: Load and synthesize with defaults
- **WHEN** a caller creates an engine, loads valid talker and codec models, and synthesizes non-empty text without request options
- **THEN** the crate SHALL return an owned synthesis result or a concrete synthesis error

#### Scenario: Synthesize before model loading
- **WHEN** a caller synthesizes text on a newly created or unloaded engine
- **THEN** the crate SHALL return a model-not-loaded error without calling the native synthesis operation

#### Scenario: Unload releases the loaded model
- **WHEN** a caller unloads a loaded engine
- **THEN** subsequent synthesis SHALL fail as unloaded and the previous native model context SHALL no longer be retained by the engine

### Requirement: Safe owned synthesis results
The crate SHALL return synthesized audio as a Rust-owned result containing f32 PCM samples and its sample rate. The result SHALL provide duration calculation and WAV-file writing without requiring callers to access native buffers or use unsafe Rust.

#### Scenario: Inspect and write successful audio
- **WHEN** synthesis succeeds
- **THEN** the caller SHALL be able to read owned samples and sample rate, compute duration, and write a valid floating-point WAV file

#### Scenario: Result outlives engine
- **WHEN** a caller retains a successful synthesis result after unloading or dropping its engine
- **THEN** the result's audio samples and WAV-writing behavior SHALL remain valid

### Requirement: Qwen inference options use safe domain types
The crate SHALL expose safe request options for supported language selection, sampling controls, and Qwen voice intent. Voice intent SHALL distinguish default output, named speakers, clone references with optional transcripts, and voice-design instructions without exposing C pointers, callbacks, or manual native-buffer ownership.

#### Scenario: Use a named speaker
- **WHEN** a caller requests a named speaker with a compatible loaded CustomVoice model
- **THEN** the engine SHALL pass that speaker selection to synthesis and return its result or a concrete native-derived error

#### Scenario: Request an incompatible voice mode
- **WHEN** a caller requests a voice intent unsupported by the loaded model
- **THEN** the crate SHALL return an error that identifies the failed synthesis or mode validation and preserves the native diagnostic when available

### Requirement: Reusable voice references are safely owned
The crate SHALL allow callers to extract a reusable voice reference from mono 24 kHz f32 PCM using a loaded compatible model. The resulting reference SHALL be usable in clone requests and SHALL release its native resources automatically when dropped.

#### Scenario: Clone with an extracted reference
- **WHEN** a caller extracts a voice reference from valid reference audio and supplies it in a compatible clone request
- **THEN** the engine SHALL synthesize using that reference or return a concrete extraction or synthesis error

#### Scenario: Drop a voice reference
- **WHEN** a voice reference is dropped without being used again
- **THEN** its native reference buffers SHALL be released without caller-managed deallocation

### Requirement: Native failures become concrete Rust errors
The crate SHALL expose a concrete error type for invalid Rust inputs, model lifecycle failures, native status failures, and native initialization failures. For a native failure with an available diagnostic, the error SHALL retain a copied diagnostic message suitable for reporting after later native calls occur.

#### Scenario: Native model load fails
- **WHEN** model loading fails in the native library
- **THEN** the crate SHALL return a concrete initialization error containing the native diagnostic when available

#### Scenario: Input contains an interior NUL
- **WHEN** a caller supplies text, a voice string, an instruction, or a model path that cannot be represented as the required C string
- **THEN** the crate SHALL return a Rust input error without invoking the native API

### Requirement: Speaker enumeration is safe
The crate SHALL allow a loaded engine to enumerate named speakers supplied by its loaded model without returning borrowed native pointers.

#### Scenario: Enumerate CustomVoice speakers
- **WHEN** a compatible loaded model exposes named speakers
- **THEN** the caller SHALL receive Rust-owned speaker names

#### Scenario: Enumerate a model without speakers
- **WHEN** a loaded Base or VoiceDesign model has no named-speaker table
- **THEN** speaker enumeration SHALL return an empty collection
