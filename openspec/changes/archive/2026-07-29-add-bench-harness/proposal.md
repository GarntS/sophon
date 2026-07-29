## Why

Sophon has no way to measure its own performance: there is no timing instrumentation anywhere in the daemon, and no tooling to quantify STT/TTS latency or round-trip times on real hardware. Users evaluating backends (CPU/CUDA/MIGraphX/Vulkan), quantization levels, or model choices currently have nothing but anecdote to go on.

## What Changes

- Add a manually-run, black-box benchmark harness (`bench/`, Python managed by `uv`) that attaches to the live session-bus daemon and reports latency statistics for the currently configured engine, model, provider, and backend.
- The harness discovers the running configuration via D-Bus lifecycle properties (no matrix to configure), waits for readiness, and measures:
  - an IPC/validation floor from pre-queue rejection calls,
  - STT end-to-end latency and real-time factor across a duration sweep via `TranscribeFile`,
  - TTS end-to-end latency and real-time factor across a text-length sweep via `SpeakToFile` and `SpeakToBuffer`.
- STT audio corpus comes from a user-supplied `--wav-dir`, with a self-generating fallback that synthesizes prompts through the daemon's own TTS and resamples them into cache.
- Results print as a percentile table (n/min/p50/p90/p99/max/mean±sd/RTF) with an optional JSONL record including an environment manifest.
- Out of scope: daemon instrumentation or code changes of any kind, `SpeakAloud`/playback-drain measurement, concurrency sweeps, cold-start matrices, backend cross-products.

## Capabilities

### New Capabilities

- `performance-benchmarks`: Manually-run black-box measurement of STT/TTS latency and round-trip times against the live daemon, including readiness discovery, an IPC baseline, input corpora with fallback generation, warmup/repetition methodology, and percentile/RTF reporting.

### Modified Capabilities

<!-- No existing spec-level requirements change; the daemon is untouched. -->

## Impact

- **New code**: `bench/` Python project (`pyproject.toml` for `uv`, `dbus-next` dependency, harness module, console entry point). No changes under `src/`, no new crate dependencies, no D-Bus API changes.
- **Dependencies**: requires a running sophon daemon on the session bus with models ready; Python + `uv` on the operator machine; `ffmpeg`/`sox` only for the corpus fallback (never a hard dependency).
- **Docs**: README section on running benchmarks and interpreting output.
