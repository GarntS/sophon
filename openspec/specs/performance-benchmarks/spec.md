## ADDED Requirements

### Requirement: Daemon discovery and readiness gating

The harness SHALL connect to the session bus and discover the running daemon configuration from its published lifecycle properties before benchmarking. It SHALL wait until the STT state is `Ready` (and `TtsState` is `Ready` when TTS benchmarks are to run), up to a configurable timeout, and SHALL record the active engine, model, TTS provider, TTS model, and TTS capabilities into the run manifest. The harness SHALL NOT start, stop, or reconfigure the daemon.

#### Scenario: Ready daemon is benchmarked as found

- **WHEN** the harness runs against a daemon whose STT and TTS states are both `Ready`
- **THEN** it records the discovered engine, model, provider, and capabilities in the manifest and proceeds without any benchmark-matrix configuration

#### Scenario: Daemon not ready within timeout

- **WHEN** the STT state does not reach `Ready` within the configured timeout
- **THEN** the harness exits nonzero, naming the observed state and the daemon's reported last error

#### Scenario: TTS unavailable but STT ready

- **WHEN** STT is `Ready` but TTS is `Failed` or does not become ready
- **THEN** the harness skips the TTS sweep with a clear notice and still reports STT and baseline results

### Requirement: IPC and validation baseline

The harness SHALL measure a transport floor by timing calls that the daemon rejects before inference is queued (including an unreadable audio path rejected as invalid audio, and an unknown TTS option key rejected as invalid options), and SHALL verify each baseline call actually fails. Baseline statistics SHALL be reported alongside, and distinguishable from, inference measurements.

#### Scenario: Baseline reflects pre-queue cost

- **WHEN** the baseline phase runs against a ready daemon
- **THEN** every timed baseline call returns the expected pre-queue error, and the report presents baseline latency separately from inference cells

#### Scenario: Unexpected baseline success is flagged

- **WHEN** a baseline call unexpectedly succeeds instead of being rejected
- **THEN** the harness flags the run as invalid for baseline purposes and excludes that measurement

### Requirement: STT latency sweep

The harness SHALL measure end-to-end `TranscribeFile` latency over a corpus spanning at least three distinct audio durations. Each cell SHALL run a configurable number of warmup calls (default 3) followed by a configurable number of measured repetitions (default 15), executed sequentially. The first call after readiness SHALL be recorded separately from steady-state measurements. Per-call real-time factor SHALL be computed as audio duration divided by call latency.

#### Scenario: Duration sweep produces per-size statistics

- **WHEN** the STT sweep completes against a ready daemon
- **THEN** each corpus duration has its own latency distribution and median RTF in the report

#### Scenario: Corpus file fails input validation

- **WHEN** a corpus file is not a 16 kHz mono WAV
- **THEN** the harness rejects that file with a message identifying the constraint violated, before any benchmark call uses it

### Requirement: STT corpus fallback generation

When no audio directory is supplied, the harness SHALL synthesize its STT corpus through the daemon's own TTS file output, convert the results to 16 kHz mono WAV using an externally detected resampler, and cache the corpus under the user's XDG cache directory for reuse across runs. A resampler SHALL be an optional runtime detection, never a hard dependency.

#### Scenario: Cached corpus is reused

- **WHEN** a previous run already generated and cached the corpus
- **THEN** the harness reuses the cached files without calling TTS or resampling again

#### Scenario: No resampler available

- **WHEN** no audio directory is supplied and neither a supported resampler nor a cached corpus is available
- **THEN** the harness skips the STT sweep with an actionable message and continues with remaining phases

### Requirement: TTS latency sweep

The harness SHALL measure end-to-end latency over at least three embedded text lengths (sentence, paragraph, page) using both `SpeakToFile` and `SpeakToBuffer`, with the same warmup/repetition methodology as the STT sweep. Per-call real-time factor SHALL be computed as call latency divided by generated audio duration, where duration derives from the call's own returned byte size.

#### Scenario: Both output modes are measured

- **WHEN** the TTS sweep runs against a ready daemon
- **THEN** file-output and buffer-output cells are reported separately for each text length

#### Scenario: Buffer mode unsupported by the bus connection

- **WHEN** descriptor passing cannot be negotiated with the bus
- **THEN** the harness skips only the buffer-output cells with a notice and still reports file-output results

### Requirement: Statistical reporting and manifest

The harness SHALL print a per-cell table of n, min, p50, p90, p99, max, mean, standard deviation, and median RTF, clearly separating warmup and first-call data from measured repetitions. When a JSON-lines output path is supplied, the harness SHALL write one record per measured call plus a manifest record containing platform information, CPU model, the discovered daemon configuration, timestamp, and harness version. All reported latencies SHALL be labeled end-to-end, with the IPC baseline identified as the transport floor.

#### Scenario: Table report after a full run

- **WHEN** all phases complete
- **THEN** the stdout report contains one row per measured cell with the full statistic set, and warmup calls never appear in measured distributions

#### Scenario: Machine-readable records

- **WHEN** the harness is invoked with a JSON-lines output path
- **THEN** the file begins with a manifest record and contains one latency record per measured call with its axis values and RTF

### Requirement: Non-interference with the daemon

The harness SHALL limit its interaction with the daemon to ordinary client method calls and property reads. Output files created during benchmarking SHALL be written to a run-scoped directory that is removed on completion unless the operator requests keeping them.

#### Scenario: Clean exit leaves no artifacts

- **WHEN** a run completes without a keep flag
- **THEN** no benchmark output files remain on disk outside the XDG corpus cache

#### Scenario: Daemon state is untouched

- **WHEN** a run completes
- **THEN** the daemon's configuration, lifecycle state, and queued work are exactly as a normal client's calls would leave them
