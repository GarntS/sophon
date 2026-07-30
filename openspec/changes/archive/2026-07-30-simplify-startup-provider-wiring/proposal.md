## Why

Startup currently carries three avoidable contradictions: a validated `cache_dir` is not passed to the model registry, loader-specific artifact roles are repeated in provider composition and checked only after resolution starts, and successful builds retain no-Qwen branches even though the crate requires exactly one Qwen backend. Aligning startup wiring with its existing contracts removes these alternate paths while preserving public behavior and interfaces.

## What Changes

- Make the validated top-level `cache_dir` the single shared root used by registry artifacts/views and TTS optimized data; retain the XDG-derived root when the field is omitted.
- Reject a relative cache root or an existing non-directory cache path before model resolution.
- Define the exact semantic artifact roles for every `LoaderKind` once, validate package manifests against that schema before any download, and reuse the schema at STT/TTS native-loader boundaries.
- Retain the central compile-time requirement for exactly one Qwen backend, but remove the six downstream Qwen availability gates and the unreachable no-backend runtime path from successful builds.
- Add focused configuration, catalog, startup-wiring, and feature-matrix checks for all three workstreams.
- Preserve all public Rust signatures/exports, D-Bus behavior, configuration and registry formats, cache layout and integrity policy, provider behavior, and Nix backend mapping.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `service-configuration`: Clarify that a valid absolute top-level cache override is the singleton registry root and that invalid roots fail before resolution.
- `model-registry`: Require each loader kind's manifest to have its exact semantic role set before model resolution begins.

## Impact

- Primary implementation: `src/main.rs`, `src/config.rs`, `src/model_registry.rs`, and `src/tts/mod.rs`.
- Supporting tests/fixtures: unit tests in `src/config.rs` and `src/model_registry.rs`, plus startup/feature checks where needed.
- Verified context, not intended for behavioral modification: `src/lib.rs`, `Cargo.toml`, `flake.nix`, `model_registry.yaml`, and existing integration tests.
- No dependency, feature-name, manifest-content, persisted-data, D-Bus, worker, provider, audio, logging, or deployment migration changes.
