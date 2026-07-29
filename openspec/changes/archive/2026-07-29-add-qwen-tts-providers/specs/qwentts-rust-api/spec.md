## ADDED Requirements

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
