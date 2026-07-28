## ADDED Requirements

### Requirement: Vendored qwentts backend library packages
The Nix flake SHALL expose named Linux qwentts shared-library packages built from the repository's pinned `third_party/qwentts.cpp` source without requiring Nix-specific modification within the submodule. Each package SHALL build only qwentts' `qwen` shared-library target, install `libqwen.so`, its required ggml runtime libraries, and `qwen.h`, and SHALL NOT install qwentts command-line tools.

#### Scenario: qwentts packages are evaluated
- **WHEN** a supported Linux system evaluates the named CPU, CUDA, or MIGraphX/Vulkan qwentts package output
- **THEN** each resolves to a derivation whose source is the repository's pinned qwentts.cpp submodule

#### Scenario: CPU package is built
- **WHEN** Nix builds the CPU qwentts package with ordinary sandboxed build steps
- **THEN** it enables CPU and OpenBLAS support and the result contains `libqwen.so`, required ggml runtime libraries, and `qwen.h`, without qwentts CLI binaries

#### Scenario: CUDA package is built
- **WHEN** Nix builds the CUDA qwentts package
- **THEN** it enables CPU and CUDA backends and installs the shared-library artifacts without qwentts CLI binaries

#### Scenario: MIGraphX/Vulkan package is built
- **WHEN** Nix builds the Sophon-named MIGraphX qwentts package
- **THEN** it enables CPU and Vulkan backends and installs the shared-library artifacts without qwentts CLI binaries

### Requirement: qwentts package validation
The flake SHALL expose deterministic checks for the CPU and MIGraphX/Vulkan qwentts library package layouts without downloading model data or performing inference. The CUDA qwentts package SHALL be evaluated but SHALL NOT be built by flake checks.

#### Scenario: qwentts packaging checks run
- **WHEN** `nix flake check` runs on a supported Linux system
- **THEN** the qwentts checks verify the required shared libraries and public header for CPU and MIGraphX/Vulkan packages and reports failure when any are missing
