## ADDED Requirements

### Requirement: The crate builds vendored qwentts.cpp
The `qwentts-cpp` crate SHALL build the pinned vendored qwentts.cpp `qwen` shared library as part of its Cargo build and SHALL generate its private bindings from the matching vendored `qwen.h` header. The crate SHALL NOT require `QWENTTS_INCLUDE_DIR` or `QWENTTS_LIB_DIR` to build.

#### Scenario: Standard CPU Cargo build
- **WHEN** a caller builds `qwentts-cpp` with its default features and the documented CPU native prerequisites are available
- **THEN** Cargo produces the Rust crate linked to a qwentts.cpp library built from the pinned vendored source

#### Scenario: Removed installed-library configuration
- **WHEN** a caller supplies or omits the former qwentts include/library environment variables
- **THEN** the crate build SHALL use its vendored source build rather than requiring those variables

### Requirement: Exactly one acceleration feature selects a native build
The crate SHALL expose `cpu`, `cuda`, `sycl`, and `vulkan` acceleration features, with `cpu` enabled by default. A build SHALL enable exactly one of these features, and the selected feature SHALL configure qwentts.cpp's corresponding acceleration target while retaining its CPU fallback support.

#### Scenario: CUDA feature is selected
- **WHEN** a caller builds with the `cuda` feature and its documented CUDA prerequisites are available
- **THEN** the build SHALL configure qwentts.cpp with CUDA acceleration and produce a crate linked to that build

#### Scenario: SYCL feature is selected
- **WHEN** a caller builds with the `sycl` feature and its documented SYCL prerequisites are available
- **THEN** the build SHALL configure qwentts.cpp with SYCL acceleration and produce a crate linked to that build

#### Scenario: Vulkan feature is selected
- **WHEN** a caller builds with the `vulkan` feature and its documented Vulkan prerequisites are available
- **THEN** the build SHALL configure qwentts.cpp with Vulkan acceleration and produce a crate linked to that build

#### Scenario: Conflicting acceleration features are selected
- **WHEN** a caller enables zero or more than one acceleration feature
- **THEN** the build SHALL fail before native compilation with an actionable diagnostic identifying the required single backend selection

### Requirement: Native build failures identify selected prerequisites
The crate build SHALL report native configuration or compilation failures in terms of the selected acceleration feature and the missing toolchain or SDK prerequisite.

#### Scenario: Selected backend prerequisite is absent
- **WHEN** a caller selects an accelerated feature without its required compiler, SDK, or library dependency
- **THEN** the build SHALL fail with a diagnostic that identifies the selected feature and the prerequisite needed to build it
