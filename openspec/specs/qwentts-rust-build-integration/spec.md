## Requirements

### Requirement: Bindings are generated from the installed public header
The crate build SHALL generate its private native bindings from qwentts.cpp's installed public `qwen.h` header. Generated raw bindings SHALL not be part of the crate's public Rust API.

#### Scenario: Build against an installed qwentts package
- **WHEN** the build receives valid qwentts header and library locations
- **THEN** it SHALL generate bindings from that header and compile the safe wrapper against those bindings

### Requirement: Shared native library discovery is explicit
The crate build SHALL obtain the qwentts header directory and shared-library directory from documented build environment configuration and SHALL dynamically link the qwentts shared library. If required native locations are absent or invalid, the build SHALL fail with an actionable error describing the missing configuration.

#### Scenario: Native locations are missing
- **WHEN** a crate build runs without the required qwentts header or library location
- **THEN** the build SHALL fail before compilation with instructions identifying the required native configuration

#### Scenario: Native locations are supplied
- **WHEN** the required qwentts header and shared-library locations are supplied
- **THEN** the build SHALL emit the native link configuration needed to link the qwentts shared library

### Requirement: Nix builds select a matching qwentts variant
The repository's Nix package builds for the new crate SHALL provide header and library locations from the qwentts CPU, CUDA, or MIGraphX package selected for that crate build. Each resulting crate package runtime closure SHALL retain access to its selected qwentts shared library and runtime dependencies. This requirement SHALL NOT add the crate as a dependency of Sophon before a TTS integration exists.

#### Scenario: CPU crate package build
- **WHEN** the CPU package for the new crate is built
- **THEN** it SHALL bind and link against the CPU qwentts package and retain its native runtime libraries

#### Scenario: Accelerator crate package build
- **WHEN** a CUDA or MIGraphX package for the new crate is built
- **THEN** it SHALL bind and link against the corresponding accelerator-enabled qwentts package rather than the CPU-only package

### Requirement: Bindgen prerequisites are reproducible in Nix
The Nix build environment for the new crate SHALL include the tools and libraries required to execute bindgen for the target build.

#### Scenario: Fresh Nix build
- **WHEN** the repository is built in a fresh Nix build environment
- **THEN** binding generation SHALL not depend on an undeclared host-installed bindgen tool or libclang installation
