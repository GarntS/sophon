## Why

`qwentts-cpp` currently requires a separately built qwentts.cpp installation and Nix-specific include/library discovery. Building the vendored native library from the crate makes ordinary Cargo builds portable beyond NixOS while retaining explicit accelerator selection.

## What Changes

- Build qwentts.cpp's `qwen` shared library from the pinned vendored source in `qwentts-cpp`'s build script instead of linking an externally installed library.
- Add mutually exclusive Cargo acceleration features for CPU, CUDA, SYCL, and Vulkan builds; select the matching qwentts.cpp CMake configuration from the feature.
- Emit native link and runtime discovery metadata from the source build, while keeping bindgen output private and generated from the same source header.
- Update Nix crate package variants to select Cargo features and provide the corresponding native build prerequisites rather than separately packaging qwentts.cpp for the crate.
- **BREAKING** Remove the crate build requirement for `QWENTTS_INCLUDE_DIR` and `QWENTTS_LIB_DIR`; builds instead require the selected backend's native toolchain.

## Capabilities

### New Capabilities
- `qwentts-rust-source-build`: Portable, feature-selected Cargo builds of the vendored qwentts.cpp native library.

### Modified Capabilities
- `nix-service-packaging`: Nix package and check requirements change from standalone qwentts library outputs to feature-selected qwentts-cpp crate builds.

## Impact

- Affects `qwentts-cpp/Cargo.toml`, `qwentts-cpp/build.rs`, native build documentation, Cargo lockfile, and `flake.nix`.
- Adds CMake/native compiler requirements to Cargo builds, with CUDA, SYCL, or Vulkan SDK/toolchain requirements for their respective features.
- Removes the crate's dependence on externally supplied qwentts include/library environment variables and may retire dedicated qwentts.cpp Nix package outputs if no other consumer requires them.
