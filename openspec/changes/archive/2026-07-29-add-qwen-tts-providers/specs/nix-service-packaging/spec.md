## MODIFIED Requirements

### Requirement: Vendored qwentts backend library packages
The Nix flake SHALL continue exposing named Linux `qwentts-cpp` package outputs for CPU, CUDA, SYCL, and Vulkan builds from the repository's pinned `third_party/qwentts.cpp` source. Each output SHALL select the matching `qwentts-cpp` Cargo acceleration feature and provide its native build and runtime prerequisites; the crate build SHALL produce the qwentts shared library and its required ggml runtime libraries. The integrated Sophon packages SHALL also depend on the matching crate backend defined by the service packaging matrix without requiring a separately packaged qwentts.cpp source build.

#### Scenario: qwentts-cpp packages are evaluated
- **WHEN** a supported Linux system evaluates the named CPU, CUDA, SYCL, or Vulkan qwentts-cpp package output
- **THEN** each resolves to a derivation selecting its corresponding Cargo feature and native dependency set

#### Scenario: CPU crate package is built
- **WHEN** Nix builds the CPU qwentts-cpp package with ordinary sandboxed build steps
- **THEN** it builds qwentts.cpp from the pinned source with CPU/OpenBLAS support and retains the resulting native runtime libraries without qwentts CLI binaries

#### Scenario: Accelerator crate package is built
- **WHEN** Nix builds a CUDA, SYCL, or Vulkan qwentts-cpp package
- **THEN** it supplies the selected accelerator's declared build prerequisites and retains only that selected accelerator runtime closure together with CPU fallback dependencies

## ADDED Requirements

### Requirement: Sophon Qwen backend matrix
Each Sophon Nix package SHALL include exactly one qwentts backend: CPU for `sophon-cpu`, CUDA for `sophon-cuda`, and Vulkan for `sophon-migraphx`. The STT backend SHALL remain CPU, CUDA, and MIGraphX respectively.

#### Scenario: CPU service is built
- **WHEN** Nix builds `sophon-cpu`
- **THEN** its service uses CPU/OpenBLAS for Qwen and its closure contains no CUDA, Vulkan, ROCm, or MIGraphX runtime introduced by Qwen

#### Scenario: CUDA service is built
- **WHEN** Nix builds `sophon-cuda`
- **THEN** both ONNX STT and Qwen TTS select CUDA and no Vulkan or ROCm runtime is introduced

#### Scenario: MIGraphX service is built
- **WHEN** Nix builds `sophon-migraphx`
- **THEN** STT selects MIGraphX while Qwen TTS selects Vulkan and the closure contains both required runtime families

#### Scenario: Conflicting Cargo backends are selected
- **WHEN** a build enables zero or multiple qwentts backend features
- **THEN** it fails before native compilation with an actionable feature-selection error

### Requirement: Installed Qwen native runtime
Integrated Sophon packages SHALL install `libqwen` and every required common and selected-backend GGML shared library, retain their OpenBLAS and accelerator dependencies, and set runtime search paths that do not reference Cargo build directories.

#### Scenario: Installed daemon starts
- **WHEN** a packaged Sophon daemon with Qwen support starts outside its Nix build directory
- **THEN** its loader resolves `libqwen` and all transitive GGML backend libraries from the package closure

#### Scenario: Package checks inspect native libraries
- **WHEN** flake checks validate a Sophon package variant
- **THEN** they verify required libraries and RPATHs and reject unrelated accelerator dependencies according to the backend matrix
