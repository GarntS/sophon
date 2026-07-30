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

### Requirement: Exclusively owned engine is movable between threads
`QwenTtsEngine` SHALL implement `Send` based on the pinned native ABI's thread-safe context contract and SHALL NOT implement `Sync`. Safe engine operations SHALL continue requiring exclusive mutable access.

#### Scenario: Engine moves to a worker
- **WHEN** a loaded engine is exclusively owned on one Rust thread and transferred to another
- **THEN** the receiving thread can continue safe synthesis and eventually drop the context

#### Scenario: Shared-reference concurrency is attempted
- **WHEN** safe Rust code attempts to share one engine by immutable reference across threads
- **THEN** compilation fails because the engine is not `Sync`

### Requirement: Native duration converts to generation tokens
The safe engine SHALL expose duration-to-token conversion for a loaded model without exposing its native context or raw status values.

#### Scenario: Provider applies an output duration
- **WHEN** a loaded engine receives a finite supported duration in seconds
- **THEN** it returns the native model's corresponding generation-token count for use as an inference cap

#### Scenario: Conversion is requested while unloaded
- **WHEN** duration conversion is called before model loading or after unloading
- **THEN** the crate returns its model-not-loaded error without invoking the native conversion

### Requirement: Safe process-wide native logging bridge
The crate SHALL expose native log levels and safe installation of a process-wide, reentrant Rust log callback whose callable is `Send`, `Sync`, and `'static`. The bridge SHALL copy each native message for the duration needed by the callback, SHALL contain Rust panics before they cross the C ABI, and SHALL support restoring default native logging.

#### Scenario: Native thread emits a message
- **WHEN** qwentts.cpp invokes the callback from any native or caller thread
- **THEN** the registered Rust callback receives the mapped level and valid UTF-8 message without borrowed storage escaping the call

#### Scenario: Rust logger panics
- **WHEN** the installed Rust callback panics while handling a native message
- **THEN** the panic is contained and never unwinds through qwentts.cpp

#### Scenario: Default logging is restored
- **WHEN** the caller clears the Rust callback
- **THEN** qwentts.cpp resumes its documented default logging behavior

### Requirement: Safe streaming Qwen synthesis
The crate SHALL expose a safe synchronous streaming synthesis operation alongside buffered synthesis. It SHALL deliver each native audio block as Rust-owned mono `f32` samples with the documented sample rate, SHALL prevent native sample pointers from escaping their callback invocation, and SHALL leave buffered `SynthesisResult` behavior unchanged.

#### Scenario: Streaming synthesis succeeds
- **WHEN** a loaded compatible engine streams valid nonempty text and the consumer accepts every chunk
- **THEN** the consumer receives owned chunks in native generation order and the operation reports successful completion without returning a duplicate buffered result

#### Scenario: Owned chunk outlives native callback
- **WHEN** the consumer retains a chunk after its callback returns
- **THEN** the chunk's samples remain valid independently of native callback storage and engine lifetime

#### Scenario: Buffered synthesis remains available
- **WHEN** a caller uses the existing buffered synthesis operation
- **THEN** it receives one complete owned `SynthesisResult` with unchanged ownership semantics

### Requirement: Safe streaming callback control
The streaming callback contract SHALL be safe to invoke from a native worker thread, SHALL support consumer-requested cancellation, SHALL contain Rust panics before they cross the C ABI, and SHALL NOT permit safe callback code to re-enter the exclusively borrowed engine.

#### Scenario: Consumer cancels a stream
- **WHEN** the Rust consumer rejects a chunk or requests cancellation
- **THEN** the native operation is cooperatively cancelled and the crate returns a concrete cancellation result distinguishable from ordinary native synthesis failure

#### Scenario: Streaming callback panics
- **WHEN** Rust callback processing panics
- **THEN** the panic is contained before returning through qwentts.cpp and the streaming call terminates with a concrete Rust error

#### Scenario: Native batching invokes the callback
- **WHEN** qwentts.cpp invokes a stream callback from an internal worker thread
- **THEN** the callback remains memory-safe and receives no reference allowing it to call back into the mutably borrowed engine
