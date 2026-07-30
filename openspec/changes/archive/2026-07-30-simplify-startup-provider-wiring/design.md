## Context

`run_async` currently initializes `ModelRegistry` with `ConfigPaths::model_cache` and only then loads `Config`, even though `Config::from_file` has already resolved the documented top-level `cache_dir` override and copied it into `TtsOperationalConfig`. This leaves registry artifacts/views on the XDG default while optimized TTS data uses the configured root.

The package catalog also carries a `LoaderKind` for every model, but the corresponding required role schemas live as four string arrays in `initialize` and `initialize_tts`. Both paths call `ModelRegistry::resolve` before `require_roles`, so an invalid package role set can begin acquisition before the loader rejects it.

Finally, `src/lib.rs` rejects zero or multiple Qwen backend features. Six later `cfg` attributes in `src/main.rs` and `src/tts/mod.rs` nevertheless model a no-Qwen successful build that cannot exist.

Constraints are strict: preserve public Rust APIs and exports, the YAML and D-Bus contracts, global registry lifetime, content verification and cache layout, provider behavior, and the CPU/CUDA/MIGraphX package matrix. External Rust consumers were not available, so no public item may be removed or have its signature changed.

## Goals / Non-Goals

**Goals:**

- Route registry artifacts/views and TTS optimized data through one validated shared cache root.
- Reject invalid top-level cache roots before any model resolution.
- Make loader-specific role sets a single catalog invariant while retaining a consumer-boundary defense.
- Remove Qwen availability branches that are impossible after the central compile guards.
- Give each of the three workstreams independent, mechanically checkable acceptance coverage inside one change.

**Non-Goals:**

- No D-Bus, provider, worker, audio, playback, logging, or model-acquisition algorithm redesign.
- No changes to Cargo feature names, dependencies, Nix package/backend mapping, `model_registry.yaml` contents, configuration keys, cache layout, or persisted data.
- No removal or signature change for `ModelRegistry::from_path`, `require_roles`, provider constructors, exports, or other public Rust APIs.
- No change to registry reload, retry, locking, hashing, hard-link publication, or lifecycle-state semantics.

## Decisions

### 1. Load the catalog once, then choose the registry cache from validated configuration

`run_async` will load `ModelCatalog` from `package_registry_path`, pass that same catalog snapshot to `Config::load_with_catalog`, and then move it into `ModelRegistry::new` for process-global initialization. This preserves exactly one package-catalog read and the existing singleton.

For `Ok(Config)`, the registry constructor receives `Config::cache_dir`. For `Err(ConfigError)`, startup retains `ConfigPaths::model_cache` only to construct the unavailable provider handles and publish the existing failed configuration lifecycle; no model resolution is started. This preserves the daemon's current invalid-configuration behavior rather than silently applying defaults.

Top-level cache validation will require an absolute path and reject an existing non-directory before `TtsConfig::from_value` and before registry construction. A nonexistent absolute path remains valid because the registry creates it on first use. Since TTS receives a clone of this same path, its Kokoro optimized graph remains beneath the shared root.

Alternatives rejected:

- Mutating or reconstructing the registry after configuration would weaken the startup-only singleton invariant.
- Parsing the package catalog once for configuration and again in `ModelRegistry::from_path` would retain duplicate I/O and two snapshots.
- Falling back to the XDG root after an explicit invalid override would violate strict configuration behavior.

### 2. Put exact role schemas on `LoaderKind`

`LoaderKind` will expose one exact role slice for each variant:

| Kind | Required roles |
|---|---|
| `Parakeet` | `encoder`, `decoder_joint`, `nemo`, `vocabulary` |
| `Canary` | `encoder`, `decoder`, `nemo`, `vocabulary` |
| `Kokoro` | `model`, `voices` |
| `Base` | `talker`, `codec` |
| `CustomVoice` | `talker`, `codec` |
| `VoiceDesign` | `talker`, `codec` |

The method must be public because `src/main.rs` is a binary crate consuming the library crate, but it only adds API; it removes none. `ModelCatalog::validate` will compare each manifest's role keys with this exact set and return `RegistryError::Invalid` on missing, extra, or wrong roles. This occurs during package-catalog loading, before an attempt is created or network work begins.

`initialize` and `initialize_tts` will call the existing public `require_roles` with `metadata.kind.required_roles()` after resolution. Keeping this defense satisfies the existing consumer contract and protects callers that construct `ModelRegistry::new` directly with a programmatic, unvalidated catalog. The literal arrays disappear from `main.rs`; Qwen role indexing remains guarded by the exact check.

Alternatives rejected:

- Removing the consumer check would make `ModelRegistry::new` callers depend on an undocumented validation precondition.
- A second typed resolved-artifact enum would change more APIs and representations than this fixed six-kind invariant requires.
- Inferring roles from model IDs or filenames would be stringly typed and less reliable than `LoaderKind`.

### 3. Treat Qwen as unconditional after the central feature guard

The two compile-time guards in `src/lib.rs` remain the sole authority: every successful build has exactly one of `qwen-cpu`, `qwen-cuda`, or `qwen-vulkan`. `src/main.rs` will define the real `install_qwen_log_bridge` directly. `src/tts/mod.rs` will compile and re-export `qwen` directly and will construct Qwen providers without the inner availability blocks or the unreachable "no Qwen backend" error.

No Cargo or Nix feature wiring changes. Existing single-backend package builds remain valid, while zero and multi-backend combinations continue to fail at the same central contract.

Alternative rejected: retaining fallback branches for `--no-default-features` is not useful because that build is deliberately rejected before it can produce a library or daemon.

### 4. Validate behavior at the narrowest boundary

- Configuration tests will cover omitted, valid absolute, relative, and existing-file cache roots and prove STT/TTS retain the same selected path.
- Registry tests will cover exact roles for all six kinds and reject missing, extra, and wrong roles through catalog parsing/validation before resolution. Existing package-catalog validation remains an acceptance check.
- Startup wiring will have a focused test seam or equivalent behavioral test proving that valid configuration selects the registry root and invalid configuration does not trigger resolution. The implementer should prefer a small pure root-selection helper over exposing registry internals if a seam is needed.
- Feature checks will compile each supported single backend in its project-native environment and assert that zero/multiple selections still fail. A source search will ensure Qwen availability `cfg` attributes remain only in the central guard (test-target gating may remain outside `src/**`).
- Run `cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --workspace`, and the applicable Nix backend checks. The audit environment could run formatting, but its missing Nix linker wrapper prevented a test verdict.

## Risks / Trade-offs

- **[Invalid role manifests fail earlier than today]** → This is intentional for package-invalid data; preserve actionable `RegistryError::Invalid` diagnostics and verify that no resolution/network attempt starts.
- **[Startup reordering could accidentally replace invalid configuration with defaults]** → Keep the `Result<Config, ConfigError>` through handle setup and use the XDG path only as an inert registry-construction root when configuration is invalid.
- **[Catalog ownership changes could lead to a second parse]** → Construct the registry with `ModelRegistry::new` from the same catalog value used for validation.
- **[Role-schema API addition could invite divergence from `require_roles`]** → Make initialization call `required_roles()` directly; do not copy role literals into provider code or tests beyond table-driven expected fixtures.
- **[Backend checks are environment-specific]** → Use the existing Cargo/Nix feature matrix and do not weaken unavailable-hardware policy or require real model inference.

## Migration Plan

No user data or configuration migration is required. Existing absolute cache overrides begin working as already documented; existing XDG caches and content-addressed layouts remain compatible. Deployment is a normal source/package update. Rollback is a source/package rollback; cache contents remain reusable because paths, fingerprints, hashes, and view formats do not change.
