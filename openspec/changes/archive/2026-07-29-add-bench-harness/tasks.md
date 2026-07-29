## 1. Project scaffolding

- [x] 1.1 Create `bench/pyproject.toml` as a `uv` project with a `dbus-next` dependency and a `sophon-bench` console entry point
- [x] 1.2 Create the `bench/` package layout and CLI skeleton with flags: `--wav-dir`, `--jsonl <path>`, `--warmup N` (default 3), `--reps N` (default 15), `--timeout SECONDS` (default 120), `--keep-outputs`
- [x] 1.3 Generate the lockfile and verify `uv run --directory bench sophon-bench --help` succeeds

## 2. D-Bus client core

- [x] 2.1 Implement async session-bus connection via `dbus-next`, negotiating Unix FD passing and recording whether descriptor transfer is available
- [x] 2.2 Implement typed wrappers for `TranscribeFile`, `SpeakToFile`, `SpeakToBuffer` (returned memfd + size), and reads of `State`, `TtsState`, `ActiveEngine`, `ActiveModel`, `ActiveTtsProvider`, `ActiveTtsModel`, `TtsCapabilities`, `LastError`, `TtsLastError`
- [x] 2.3 Map daemon D-Bus error names to structured harness exceptions so rejection calls can be verified by error type

## 3. Discovery and readiness

- [x] 3.1 Implement readiness polling of `State` until `Ready` (or `Failed`), exiting nonzero on timeout while naming the observed state and last error
- [x] 3.2 Implement the same readiness handling for `TtsState`, degrading to "skip TTS sweep with notice" instead of aborting when STT is ready
- [x] 3.3 Implement manifest capture: discovered engine/model/provider/capabilities plus uname, CPU model, timestamp, and harness version

## 4. Baseline phase

- [x] 4.1 Implement timed baseline calls: `TranscribeFile` on a nonexistent path (expect invalid-audio rejection) and `SpeakToFile` with an unknown option key (expect invalid-options rejection)
- [x] 4.2 Verify every baseline call fails with the expected pre-queue error type; flag and exclude any measurement that unexpectedly succeeds

## 5. STT corpus

- [x] 5.1 Implement `--wav-dir` loading: accept only 16 kHz mono WAV files, reject others with a message naming the violated constraint, and bucket accepted files by duration (targets ~1s/3s/10s/30s)
- [x] 5.2 Implement fallback synthesis: fixed prompt sentences through `SpeakToFile`, runtime detection of `ffmpeg`/`sox`, and resample of the 24 kHz output to 16 kHz mono WAV
- [x] 5.3 Implement XDG cache storage for the generated corpus with reuse on later runs, and the skip-STT-with-actionable-message path when neither `--wav-dir`, cache, nor a resampler is available

## 6. Measurement sweeps

- [x] 6.1 Implement the shared measurement loop: `time.monotonic_ns` around each awaited call, sequential execution, warmup calls recorded separately, first post-ready call recorded separately, per-cell progress output, and partial results preserved on interrupt
- [x] 6.2 Implement the STT sweep over corpus durations with per-call RTF = audio duration / call latency
- [x] 6.3 Implement the TTS sweep over the three embedded text lengths × {`SpeakToFile`, `SpeakToBuffer`}, with per-call RTF = call latency / (returned bytes ÷ 96 000 B/s), skipping buffer cells when FD passing is unavailable

## 7. Reporting

- [x] 7.1 Implement per-cell statistics (n, min, p50, p90, p99, max, mean, stddev, median RTF) and a stdout table that excludes warmup/first-call data from measured distributions and labels all latencies end-to-end with the baseline identified as the transport floor
- [x] 7.2 Implement `--jsonl` output: manifest record first, then one record per measured call with axis values, latency, and RTF
- [x] 7.3 Implement the run-scoped output directory: all generated speech files land there and it is removed on completion unless `--keep-outputs` is passed

## 8. Docs and validation

- [x] 8.1 Add a README section covering how to run benchmarks, corpus options, output interpretation, and caveats (idle machine, end-to-end labeling, Qwen seed reproducibility note)
- [x] 8.2 Do a manual smoke run against a live daemon and attach the example report to the change directory for review
- [x] 8.3 Run `openspec validate add-bench-harness` and confirm the repo's existing checks (`cargo fmt`, `clippy`, `cargo test`) are unaffected
