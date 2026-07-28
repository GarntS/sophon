## Context

The completed `qwentts-cpp` crate generates bindings from an installed qwentts.cpp header and dynamically links an installed `libqwen`. The flake currently builds those native libraries independently for CPU, CUDA, and a Vulkan configuration named MIGraphX. That model is reproducible on NixOS but prevents a normal Cargo build from producing the required native library.

The vendored qwentts.cpp CMake project supports CPU, CUDA, SYCL, and Vulkan through `GGML_*` options. The Rust wrapper needs exactly one selected acceleration backend per build, while CPU support remains present as qwentts.cpp's fallback backend.

## Goals / Non-Goals

**Goals:**
- Build the pinned vendored qwentts.cpp `qwen` shared library during a `qwentts-cpp` Cargo build.
- Expose mutually exclusive CPU, CUDA, SYCL, and Vulkan Cargo features, with CPU as the default.
- Generate bindings from the same vendored header and link the produced native artifacts without `QWENTTS_INCLUDE_DIR` or `QWENTTS_LIB_DIR`.
- Make Nix select the crate feature and declare the native tools, SDKs, and runtime closure required by that feature.

**Non-Goals:**
- Support qwentts.cpp backends beyond CPU, CUDA, SYCL, and Vulkan in this change.
- Build qwentts.cpp command-line tools, download models, or change the safe Rust API.
- Support multiple accelerator features in a single crate artifact.
- Keep standalone qwentts.cpp Nix package outputs solely for the crate; other consumers, if any, must be assessed before removal.

## Decisions

### Build qwentts.cpp from the vendored source in `build.rs`

The build script will configure and build only the CMake `qwen` target into Cargo's build output directory. It will use the vendored `third_party/qwentts.cpp/src/qwen.h` as bindgen input, emit rerun triggers for the native source/configuration and Cargo features, and emit link-search/link-library directives for `qwen` and its produced ggml dependencies.

This gives Cargo one self-contained native build model across Nix and non-Nix environments. The alternative—retaining an installed-library mode—reintroduces two native build paths and environment-variable-specific behavior.

### Select exactly one backend with Cargo features

`cpu` will be the default feature. `cuda`, `sycl`, and `vulkan` will be opt-in alternatives; the build script will fail early with an actionable message if zero or more than one backend feature is enabled. Each feature maps to qwentts.cpp's corresponding CMake option (`GGML_CUDA`, `GGML_SYCL`, or `GGML_VULKAN`), while every build keeps its normal CPU fallback support.

Cargo features are selected before native compilation and naturally propagate through Nix's Cargo invocations. The alternative of runtime backend discovery would make package closures and native prerequisites ambiguous.

### Let each environment supply only its selected native prerequisites

The build script will invoke CMake and report missing toolchain/SDK configuration in terms of the selected feature. Nix will provide CMake, a C/C++ compiler, bindgen/libclang, OpenBLAS, and only the selected accelerator's build/runtime dependencies. SYCL Nix packaging will be implemented only after selecting a nixpkgs-supported SYCL toolchain and documenting its required compiler/SDK inputs.

This preserves portable Cargo defaults without hard-coding Nix paths or attempting to provision proprietary/vendor SDKs from the build script.

### Replace crate-specific standalone native Nix packages

The dedicated qwentts-cpp CPU/CUDA/Vulkan Nix outputs will build the crate with the matching feature. The existing standalone qwentts outputs and artifact-layout checks will be removed if no remaining consumer depends on them; replacement checks will verify CPU and Vulkan crate builds, and evaluate CUDA and SYCL variants without requiring physical accelerator hardware.

## Risks / Trade-offs

- [Every Cargo build compiles C++/ggml] → Use Cargo's per-target build output and CMake incremental build directory; document the added tool requirements.
- [CUDA, SYCL, or Vulkan SDKs are absent] → Fail before compilation with the selected feature and missing prerequisite in the diagnostic.
- [Feature unification enables conflicting backend flags in a dependency graph] → Detect and reject all non-single-backend combinations in `build.rs`.
- [Nix lacks a viable SYCL toolchain] → Make SYCL evaluation/check work conditional on an explicitly supported nixpkgs toolchain; do not mislabel Vulkan as SYCL or MIGraphX.
- [Native runtime dependencies are omitted] → Add feature-specific runtime-closure checks for built Nix variants.

## Migration Plan

1. Add the source-build feature model and validate the CPU Cargo build first.
2. Convert Nix crate outputs one backend at a time, preserving CPU/CUDA/Vulkan coverage and adding SYCL once its toolchain is established.
3. Remove environment-variable documentation and unused standalone qwentts outputs after confirming no consumer remains.
4. Roll back by restoring the existing installed-library build script and Nix native-package wiring; Rust API consumers require no source changes.
