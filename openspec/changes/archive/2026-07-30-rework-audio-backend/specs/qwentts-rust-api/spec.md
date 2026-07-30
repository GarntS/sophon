## ADDED Requirements

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
