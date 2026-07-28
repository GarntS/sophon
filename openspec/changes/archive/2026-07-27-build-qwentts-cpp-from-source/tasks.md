## 1. Cargo feature and source-build foundation

- [x] 1.1 Add `cpu` (default), `cuda`, `sycl`, and `vulkan` crate features plus the build dependencies needed to configure and build vendored qwentts.cpp.
- [x] 1.2 Implement early build-script validation that requires exactly one acceleration feature and identifies selected-backend prerequisite failures.
- [x] 1.3 Replace installed-header/library discovery with a Cargo-target-specific CMake build of the pinned `third_party/qwentts.cpp` `qwen` target, mapping each feature to its qwentts.cpp CMake flags.
- [x] 1.4 Generate private bindings from the vendored `qwen.h`, emit native dependency/link directives for the generated qwen/ggml libraries, and add source, feature, and toolchain rebuild triggers.
- [x] 1.5 Remove the `QWENTTS_INCLUDE_DIR` and `QWENTTS_LIB_DIR` build contract and update tests to cover default, single-feature, and conflicting-feature validation paths.

## 2. Portable native-build verification and documentation

- [x] 2.1 Add build/test coverage that validates the default CPU source build and its native runtime-library discovery without model downloads.
- [x] 2.2 Add feature-gated configuration coverage for CUDA, SYCL, and Vulkan, including actionable diagnostics when each selected toolchain or SDK is unavailable.
- [x] 2.3 Update crate documentation with Cargo build prerequisites, feature selection examples, mutually exclusive feature behavior, and the removal of external qwentts discovery variables.

## 3. Nix feature-selected packages

- [x] 3.1 Inventory consumers of the standalone qwentts Nix outputs and remove or retain them only where an independent consumer still requires them.
- [x] 3.2 Refactor Nix qwentts-cpp CPU, CUDA, SYCL, and Vulkan package outputs to build the crate from its vendored source with the matching Cargo feature and declared CMake/compiler/bindgen/OpenBLAS prerequisites.
- [x] 3.3 Determine and declare the nixpkgs-supported SYCL compiler/SDK and runtime closure required by the SYCL crate variant; fail evaluation with an actionable explanation if no supported toolchain is available.
- [x] 3.4 Supply CUDA and Vulkan build/runtime dependencies only to their selected variants, preserve CPU fallback support, and ensure no qwentts CLI binaries are packaged.
- [x] 3.5 Replace standalone qwentts artifact checks with CPU/Vulkan crate package and runtime-closure checks; evaluate CUDA/SYCL variants without requiring hardware.

## 4. Validation

- [x] 4.1 Run formatting, crate tests, default CPU source build, and applicable feature configuration checks.
- [x] 4.2 Run applicable Nix package/check builds and validate the OpenSpec change in strict mode.
