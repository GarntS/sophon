## Requirements

### Requirement: MIGraphX accelerator selection
The service SHALL support `migraphx` as an accelerator configuration value when the installed package includes the MIGraphX execution provider. It SHALL configure MIGraphX before constructing any ONNX model sessions. An explicit `migraphx` request in a package without that provider SHALL fail model initialization; `auto` in a MIGraphX-enabled package SHALL retain CPU fallback.

#### Scenario: MIGraphX package is explicitly selected
- **WHEN** Sophon runs from a MIGraphX-enabled package with `accelerator: migraphx`
- **THEN** it initializes ONNX model sessions with the MIGraphX provider before CPU fallback

#### Scenario: MIGraphX is requested from another package
- **WHEN** Sophon runs from a CPU or CUDA package with `accelerator: migraphx`
- **THEN** startup fails with an actionable unavailable-accelerator error

### Requirement: Obsolete ROCm configuration rejection
The service SHALL reject `accelerator: rocm` as an obsolete configuration value and SHALL NOT interpret it as MIGraphX.

#### Scenario: Configuration uses the obsolete value
- **WHEN** the startup YAML specifies `accelerator: rocm`
- **THEN** configuration validation fails and identifies the accelerator value as invalid

### Requirement: Separate MIGraphX Nix package
The flake SHALL expose `sophon-migraphx`, built with the MIGraphX provider and a nixpkgs-provided ONNX Runtime configured for MIGraphX. The default CPU package and CUDA package SHALL not depend on the MIGraphX/ROCm runtime closure solely because of Sophon.

#### Scenario: MIGraphX package is built
- **WHEN** a supported NixOS system builds `packages.<system>.sophon-migraphx`
- **THEN** the package dynamically loads its ONNX Runtime and can register the MIGraphX provider

#### Scenario: CPU and CUDA package closures are inspected
- **WHEN** closure-policy checks inspect the CPU and CUDA Sophon packages
- **THEN** neither package contains MIGraphX or ROCm dependencies introduced by Sophon

### Requirement: Nixpkgs ONNX Runtime sourcing
Every Sophon Nix package SHALL source its ONNX Runtime from nixpkgs and SHALL not fetch a Microsoft ONNX Runtime release archive during evaluation or build.

#### Scenario: A package builds in a sandbox
- **WHEN** Nix builds any Sophon package with ordinary build-step network access disabled
- **THEN** the Rust build dynamically links the selected nixpkgs ONNX Runtime without an ORT runtime download
