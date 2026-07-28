## ADDED Requirements

### Requirement: Nix flake outputs
The repository SHALL provide a Nix flake with Linux package outputs for CPU, CUDA, and ROCm Sophon variants, with the CPU package as the default, plus a default app, development shell, and automated checks.

#### Scenario: Default package is evaluated
- **WHEN** a supported Linux system evaluates `packages.<system>.default`
- **THEN** it resolves to the CPU Sophon package without a CUDA or ROCm runtime closure

#### Scenario: GPU variant is evaluated
- **WHEN** a supported Linux system evaluates the CUDA or ROCm package output
- **THEN** it resolves to a Sophon build containing only the requested GPU provider and CPU fallback

### Requirement: Reproducible ONNX Runtime packaging
Every package variant SHALL obtain Rust dependencies and its provider-compatible ONNX Runtime entirely through pinned Nix inputs or fixed-output sources and SHALL NOT require network access from a sandboxed build step.

#### Scenario: CPU package builds in a sandbox
- **WHEN** Nix builds the CPU package with network access disabled for ordinary build steps
- **THEN** ONNX Runtime is linked or loaded from Nix-provided artifacts and the build succeeds without an `ort-sys` runtime download

#### Scenario: Runtime compatibility is checked
- **WHEN** package checks run for a variant
- **THEN** they verify that the ONNX Runtime API version is compatible and that the expected execution provider can be registered

### Requirement: Per-user D-Bus activation metadata
Each Sophon package SHALL install a D-Bus service activation file under `share/dbus-1/services` for `com.garntresearch.sophon`, executing the Sophon daemon from that package.

#### Scenario: Client triggers activation
- **WHEN** one Sophon package variant is installed into the user environment and a session-bus client addresses `com.garntresearch.sophon`
- **THEN** D-Bus starts that package's daemon without requiring a GUI or privileged system service

### Requirement: Headless runtime closure
The CPU package SHALL not depend on GUI toolkits, display servers, desktop portals, audio capture servers, CUDA, or ROCm. GPU variants SHALL add only dependencies needed by their selected provider.

#### Scenario: CPU runtime dependencies are inspected
- **WHEN** the CPU package closure is analyzed
- **THEN** no graphical or vendor GPU runtime is present solely for Sophon

### Requirement: Flake validation checks
The flake SHALL expose checks covering Rust formatting, static analysis, unit/integration tests, D-Bus interface behavior, configuration validation, model-cache safety, and Nix package evaluation/build viability where supported.

#### Scenario: Flake checks run
- **WHEN** `nix flake check` runs on a supported Linux system
- **THEN** deterministic tests that do not require physical GPU hardware or large model downloads execute and report failure on contract regressions

### Requirement: Vendored qwentts backend library packages
The Nix flake SHALL expose named Linux `qwentts-cpp` package outputs for CPU, CUDA, SYCL, and Vulkan builds from the repository's pinned `third_party/qwentts.cpp` source. Each output SHALL select the matching `qwentts-cpp` Cargo acceleration feature and provide its native build and runtime prerequisites; the crate build SHALL produce the qwentts shared library and its required ggml runtime libraries. The flake SHALL NOT require a separately packaged qwentts.cpp library for these crate outputs, and SHALL NOT add qwentts-cpp as a dependency of the Sophon service before a TTS integration exists.

#### Scenario: qwentts-cpp packages are evaluated
- **WHEN** a supported Linux system evaluates the named CPU, CUDA, SYCL, or Vulkan qwentts-cpp package output
- **THEN** each resolves to a derivation selecting its corresponding Cargo feature and native dependency set

#### Scenario: CPU crate package is built
- **WHEN** Nix builds the CPU qwentts-cpp package with ordinary sandboxed build steps
- **THEN** it builds qwentts.cpp from the pinned source with CPU/OpenBLAS support and retains the resulting native runtime libraries without qwentts CLI binaries

#### Scenario: Accelerator crate package is built
- **WHEN** Nix builds a CUDA, SYCL, or Vulkan qwentts-cpp package
- **THEN** it supplies the selected accelerator's declared build prerequisites and retains only that selected accelerator runtime closure together with CPU fallback dependencies

### Requirement: qwentts package validation
The flake SHALL expose deterministic checks for CPU and Vulkan qwentts-cpp package builds without downloading model data or performing inference. CUDA and SYCL package variants SHALL be evaluated by checks but SHALL NOT require physical accelerator hardware or be built by the default check suite.

#### Scenario: qwentts-cpp packaging checks run
- **WHEN** `nix flake check` runs on a supported Linux system
- **THEN** it verifies CPU and Vulkan qwentts-cpp package build viability and native runtime closure presence, and evaluates the CUDA and SYCL package definitions
