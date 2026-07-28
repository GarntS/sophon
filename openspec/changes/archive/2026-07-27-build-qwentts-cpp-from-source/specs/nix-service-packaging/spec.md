## MODIFIED Requirements

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
