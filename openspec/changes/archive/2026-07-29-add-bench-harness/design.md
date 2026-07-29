## Context

Sophon is a headless session D-Bus daemon providing STT (`TranscribeFile`/`TranscribeMemfd`) and TTS (`SpeakToFile`/`SpeakToBuffer`/`SpeakAloud`). Every call flows: D-Bus dispatch → option decode/validation → bounded FIFO queue (single serialized worker thread per direction) → synchronous model inference → output tail (WAV encode, exclusive file publish, or sealed memfd + FD transfer). There is currently **no timing instrumentation anywhere** in that path, and no tooling to measure end-to-end latency.

Key constraints discovered during exploration:

- The daemon and its clients share one host, hence one monotonic clock — client-side wall-clock measurement is exact for this use case.
- Pre-queue rejection paths (`InvalidAudio`, `InvalidTtsOptions`, `NotReady`) return before inference is queued, so timing them yields an IPC + validation floor for free.
- Both workers are serialized FIFOs (KNOWN_ISSUES documents Qwen head-of-line blocking); a sequential, single-client harness avoids queue wait polluting inference numbers by design.
- No streaming exists, so latency is always whole-call; the natural normalized metric is real-time factor (RTF).
- The daemon's WAV input validation is strict: 16 kHz mono (`audio.rs`), which constrains corpus generation.

This design covers a manually-run, black-box benchmark harness living in `bench/`. The daemon is untouched.

## Goals / Non-Goals

**Goals:**

- One command (`uv run sophon-bench`) produces a percentile latency report for whatever configuration the live daemon is running — nothing to configure.
- Measure: IPC/validation baseline, STT e2e latency + RTF over a duration sweep, TTS e2e latency + RTF over a text-length sweep, via both file and buffer output modes.
- Self-contained corpus: user-supplied `--wav-dir`, else synthesized through the daemon's own TTS and cached.
- Human-readable table output plus machine-readable `--jsonl` records with an environment manifest.
- Honest methodology: warmup separated from measured reps, first post-ready call reported on its own, monotonic clock, per-call RTF.

**Non-Goals:**

- No daemon code changes or server-side phase instrumentation (queue-wait vs. inference split stays blended; deferred to a possible future change).
- No `SpeakAloud` / PipeWire playback-drain measurement.
- No concurrency sweeps, queue-saturation tests, or cold-start/backend matrices.
- Not a CI regression gate — this is a manual, operator-run tool.
- No model downloads or daemon lifecycle management; the harness never starts or restarts sophon.

## Decisions

### 1. Black-box client over instrumentation

Measure from the client side only. Rationale: zero daemon changes, measures exactly what callers experience, and the phase split it forfeits (queue wait vs. inference) is irrelevant for a sequential single-client run where queue wait is ~zero by construction. The IPC floor is recovered by timing pre-queue rejection calls. Alternatives considered: adding `Instant` probes to `worker.rs`/`tts.rs` (rejected for now — requires daemon changes and a serialization story for the timings; noted as a future thread); criterion micro-benchmarks (rejected — needs real multi-GB models and skips the D-Bus path entirely).

### 2. Python + `uv` + `dbus-next`

The harness is a real script, not a compiled artifact. `dbus-next` is pure Python and negotiates Unix FD passing, so `TranscribeMemfd`-style methods and `SpeakToBuffer` (returns a memfd) work without system GLib. `uv` owns the venv and lockfile. Alternatives considered: bash + `gdbus`/`busctl` (cannot pass FDs — would drop buffer-mode methods); `dasbus` (needs system pygobject/GLib — painful under `uv` on NixOS); a Rust bin (fits the repo's main stack but is a compiled tool, not the requested script, and adds build time to a manual diagnostic).

### 3. Introspect the running daemon instead of configuring a matrix

The harness reads `State`/`TtsState` (polled until `Ready`, with timeout), `ActiveEngine`, `ActiveModel`, `ActiveTtsProvider`, `ActiveTtsModel`, and `TtsCapabilities` from the bus and benchmarks exactly that. Rationale: the user's goal is "stats for what I have built and running", and capability discovery lets the harness skip impossible cells (e.g., named-voice requests on a provider without `named-voices`) rather than fail mid-run. Alternative: a configurable model/backend matrix (rejected — combinatorial, and the daemon can only run one config at a time anyway).

### 4. Corpus: `--wav-dir` with self-generating fallback

STT inputs are 16 kHz mono WAVs of ~1s/3s/10s/30s. If `--wav-dir` is given, files are validated against the daemon's own input constraints (16 kHz mono WAV) and bucketed by duration. Otherwise the harness synthesizes fixed prompt sentences via `SpeakToFile` (24 kHz f32 output), resamples to 16 kHz mono with `ffmpeg` or `sox` if one is on PATH, and caches the result under `$XDG_CACHE_HOME/sophon/bench-corpus` for reuse. Rationale: hermetic by default, no binary assets committed, no hard dependency on a resampler. TTS inputs are fixed texts at three lengths (sentence / paragraph / page), embedded in the harness.

### 5. Methodology: warmup, modest reps, per-call RTF

The first post-ready inference in each STT/TTS sweep is recorded separately as a cold-ish data point. Per cell, that is followed by 3 warmup calls and ~15 measured repetitions (configurable via flags); later cells run the warmups and measured repetitions without mislabeling another call as post-ready. Clock is `time.monotonic_ns` around the awaited D-Bus call. RTF is computed per call, not from aggregates: STT `rtf = audio_seconds / latency_seconds`; TTS `rtf = latency_seconds / audio_seconds`, where TTS audio seconds derive from each call's returned `size_bytes` (24 kHz × 4 B × mono = 96 000 B/s, minus WAV header). Per-call RTF keeps Qwen's nondeterministic generation length from skewing results; the report notes that a configured `seed` makes TTS runs more reproducible. Cells run strictly sequentially.

### 6. Output: stdout table + optional JSONL with manifest

Every cell reports n, min, p50, p90, p99, max, mean, stddev, and median RTF. `--jsonl <path>` writes one record per measured call (axis values + latency + RTF) headed by a manifest record: uname, CPU model, discovered engine/model/provider/capabilities, timestamp, harness version. Rationale: the table answers "how fast is my setup"; JSONL keeps the door open to tracking numbers over time without committing to a dashboard now.

## Risks / Trade-offs

- **Self-generated corpus needs TTS Ready and a resampler** → If TTS is unavailable/failed, or neither `ffmpeg` nor `sox` exists, skip the STT sweep with an actionable message pointing at `--wav-dir`; TTS sweep still runs.
- **A busy or initializing daemon stalls the run** → Readiness poll has a timeout (default 120s, configurable); on timeout or `Failed` state, exit nonzero naming the offending state and `LastError`.
- **System noise inflates variance (CPU governor, background load)** → Warmup + distribution reporting (p50/p90/p99, not just mean), and the report prints a "run on an idle machine" caveat; acceptable for a manual tool.
- **`dbus-next` FD negotiation could fail on exotic bus setups** → Detect and skip only the `SpeakToBuffer` cells; file-mode results still stand.
- **Long TTS texts on CPU Qwen are slow (minutes per call)** → "Page" length is capped (~1 500 chars), reps are configurable, and the harness prints per-cell progress so an operator can Ctrl-C with partial results intact.
- **Black-box can't attribute latency** → Accepted: the report explicitly labels all numbers as end-to-end and notes the IPC baseline as the transport floor; deeper attribution is a documented future change.

## Migration Plan

Purely additive: new `bench/` directory, README section, no changes to `src/`, packaging, or the D-Bus API. Rollback is deleting the directory. Nothing existing is affected.

## Open Questions

- Exact prompt text/sentence content for the embedded corpus (any natural text works; tune during implementation).
- Whether JSONL output later feeds a tracking dashboard — schema is deliberately simple so this can be decided later.
- Whether a future change adds daemon phase probes; if it does, this harness's JSONL records already have room for server-reported phase fields.
