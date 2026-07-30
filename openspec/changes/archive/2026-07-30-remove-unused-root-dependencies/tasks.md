## 1. Confirm cross-change prerequisite

- [x] 1.1 Apply `harden-audio-file-ingestion` first or confirm `src/audio.rs` directly references `libc::O_NONBLOCK`.
- [x] 1.2 Re-run a first-party search across `src`, `tests`, and `crates/qwentts-cpp` confirming no Rust/build-code references to `async_trait`, `flate2`, `serde_json`, `tar`, `tracing_subscriber`, or `xz2`.

## 2. Remove unsupported declarations

- [x] 2.1 Delete exactly `async-trait`, `flate2`, `serde_json`, `tar`, `tracing-subscriber`, and `xz2` from the root `Cargo.toml` `[dependencies]` table.
- [x] 2.2 Keep direct pinned `libc` and add no replacement dependency or feature change.
- [x] 2.3 Refresh `Cargo.lock` offline using the existing Cargo toolchain.
- [x] 2.4 Inspect the manifest and lockfile diff, accepting only intended declaration/reachability removals and rejecting version changes to still-reachable packages.

## 3. Verify the resolved graph

- [x] 3.1 Run offline reverse trees confirming `async-trait`, `flate2`, and `serde_json` remain only where existing transitive dependencies require them.
- [x] 3.2 Confirm `tar`, `tracing-subscriber`, `xz2`, and any packages reachable only through them are absent from the workspace graph.
- [x] 3.3 Confirm no file other than root `Cargo.toml` and `Cargo.lock` was changed by this cleanup itself.

## 4. Run project validation

- [x] 4.1 Run `nix develop -c cargo fmt --all -- --check`.
- [x] 4.2 Run `nix develop -c cargo clippy --all-targets -- -D warnings`.
- [x] 4.3 Run `nix develop -c cargo test --workspace`.
- [x] 4.4 Run `nix build .#checks.x86_64-linux.sophon-cpu-qwen-runtime` and `nix build .#checks.x86_64-linux.dbus-activation`.
- [x] 4.5 On backend-capable builders, run the CUDA and MIGraphX runtime checks; otherwise record them as required CI validation. (MIGraphX runtime check passed locally; CUDA remains required CI validation because no NVIDIA tooling is available.)
- [x] 4.6 Run the full `nix flake check` where the documented backend-capable environment is available. (Attempted locally; CUDA dependency build requires unavailable `nvcc`. MIGraphX runtime validation passed; full CUDA-capable CI remains required.)
