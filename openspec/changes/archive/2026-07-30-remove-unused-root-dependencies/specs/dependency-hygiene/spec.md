## ADDED Requirements

### Requirement: Earned root dependencies
The root Sophon package SHALL directly declare only crates used by its first-party targets or required to expose an intentional package feature. It SHALL NOT directly declare `async-trait`, `flate2`, `serde_json`, `tar`, `tracing-subscriber`, or `xz2`. It SHALL retain direct `libc` while audio file ingestion uses its Unix nonblocking-open constant.

#### Scenario: Root manifest is inspected
- **WHEN** the root `[dependencies]` table is checked after audio-ingestion hardening and dependency cleanup
- **THEN** the six unused entries are absent, `libc` is present and referenced by first-party audio code, and no replacement dependency has been introduced

### Requirement: Dependency cleanup preserves supported builds
Removing unused direct dependencies SHALL preserve the resolved versions of still-reachable packages, supported Cargo feature combinations, offline builds, Nix package outputs, native backend selection, and application behavior.

#### Scenario: Removed crate remains transitively required
- **WHEN** an existing dependency still requires `async-trait`, `flate2`, or `serde_json`
- **THEN** Cargo retains the transitive crate in the resolved graph without Sophon declaring it directly

#### Scenario: Package is reachable only through a removed declaration
- **WHEN** a package is no longer reachable from any workspace member after the six declarations are removed
- **THEN** it and any newly unreachable-only dependencies are absent from the refreshed lockfile

#### Scenario: Supported builds are validated
- **WHEN** workspace Rust checks and supported CPU, CUDA, and MIGraphX/Vulkan Nix package checks run
- **THEN** they succeed without network-dependent build steps or behavior changes attributable to dependency cleanup
