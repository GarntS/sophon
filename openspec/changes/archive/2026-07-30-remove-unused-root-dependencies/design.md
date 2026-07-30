## Context

An exhaustive first-party search found no Rust source, test, or workspace-member build-code references to `async-trait`, `flate2`, `serde_json`, `tar`, `tracing-subscriber`, or `xz2`. Offline reverse trees show the first three remain required transitively, while `tar`, `tracing-subscriber`, and `xz2` are currently rooted only by Sophon. The original audit also identified `libc`, but the selected audio-ingestion hardening now requires `libc::O_NONBLOCK`; the user explicitly chose to retain it rather than add another dependency or accept FIFO blocking.

## Goals / Non-Goals

**Goals:**

- Remove exactly the six unsupported direct declarations.
- Remove only newly unreachable lockfile packages while preserving reachable versions.
- Preserve every supported Cargo feature, Nix package/check, and runtime behavior.
- Keep `libc` as an earned direct dependency coordinated with audio hardening.

**Non-Goals:**

- No dependency upgrades, substitutions, feature changes, license-policy changes, or source refactors.
- No removal of transitive crates still required by dependencies.
- No edits outside root `Cargo.toml` and `Cargo.lock` during this change's implementation.
- No attempt to clean vendored manifests or benchmark Python dependencies.

## Decisions

1. **Remove exactly six root entries.** Delete `async-trait`, `flate2`, `serde_json`, `tar`, `tracing-subscriber`, and `xz2` from `[dependencies]`. Do not remove `libc`, and do not add replacement packages.

2. **Treat first-party references and Cargo's resolved graph as separate evidence.** Source search establishes that no Sophon target directly imports the six crates. `cargo tree --offline -p sophon -i <crate>` establishes whether each crate remains transitively reachable. It is correct for `async-trait`, `flate2`, and `serde_json` to remain in `Cargo.lock`; direct-dependency cleanup does not require erasing transitive use.

3. **Update the lockfile without opportunistic upgrades.** After editing the manifest, run Cargo metadata/check offline to refresh reachability. Inspect `Cargo.lock` and reject version changes to still-reachable packages; the intended lockfile change is removal of packages no longer reachable from any workspace member. If the local Cargo command proposes upgrades, restore those versions before accepting the lockfile.

4. **Validate all packaging paths.** The manifest participates in workspace tests and Nix builds for CPU, CUDA, and MIGraphX/Qwen Vulkan variants. Standard Rust validation plus package/runtime checks are required even though no source behavior changes.

5. **Coordinate with audio hardening.** Apply `harden-audio-file-ingestion` first or in the same implementation series. A direct search after both changes must find `libc::O_NONBLOCK` in `src/audio.rs`; this cleanup must not revert that decision.

## Risks / Trade-offs

- **[Risk] A crate is used through a macro or feature path missed by text search.** → Compile all targets and supported backend packages; Cargo will report any missing direct crate.
- **[Risk] Lockfile refresh upgrades unrelated packages.** → Accept only reachability removals and retain versions/checksums for still-reachable packages.
- **[Risk] Accelerator-only builds depend on a removed direct entry.** → Run/evaluate CPU, CUDA, and MIGraphX package checks with their normal feature sets.
- **[Trade-off] `libc` remains direct despite being unused in the pre-change tree.** → It is required by the approved file-ingestion design and is already pinned/transitive.

## Migration Plan

1. Apply `harden-audio-file-ingestion` or confirm its `libc::O_NONBLOCK` use is present.
2. Remove the six manifest entries and refresh `Cargo.lock` offline.
3. Inspect the manifest/lock diff for only intended removals and no reachable-version changes.
4. Run source searches, reverse trees, Rust validation, and Nix package checks.

Rollback restores the six manifest entries and prior lockfile. There is no runtime or data migration.
