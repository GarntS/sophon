## Why

The root package directly declares six crates that are not referenced by first-party source, tests, or build code. Removing those unsupported declarations reduces manifest and build-graph weight while preserving transitive crates where actual dependencies still require them.

## What Changes

- Remove direct root dependencies `async-trait`, `flate2`, `serde_json`, `tar`, `tracing-subscriber`, and `xz2` from `Cargo.toml`.
- Regenerate `Cargo.lock` using the existing toolchain so packages reachable only through those declarations leave the graph.
- Retain direct `libc`: the selected `harden-audio-file-ingestion` change uses `libc::O_NONBLOCK` to reject FIFOs safely after a single open.
- Add no replacement packages and make no source, feature, runtime, packaging-policy, or public-interface changes as part of this cleanup.

## Capabilities

### New Capabilities

- `dependency-hygiene`: Build-graph requirements ensuring direct root dependencies are earned by first-party use and cleanup preserves all supported builds.

### Modified Capabilities

None. Existing service and packaging behavior remains unchanged.

## Impact

- Files changed during implementation: root `Cargo.toml` and generated `Cargo.lock` only.
- Offline reverse-tree evidence: `tar`, `tracing-subscriber`, and `xz2` are currently reachable only from Sophon's direct declarations; `async-trait`, `flate2`, and `serde_json` remain transitively reachable through existing dependencies.
- `libc` remains direct and becomes used by `src/audio.rs` when `harden-audio-file-ingestion` is applied.
- Must preserve Cargo/Nix offline builds, all Qwen backend feature combinations, native runtime packaging, licenses, and application behavior.
- Ordering: apply `harden-audio-file-ingestion` before or together with this cleanup; if applied first, this change must still retain `libc` based on the recorded cross-change decision.
