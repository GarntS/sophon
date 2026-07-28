## Context

Sophon already records `third_party/qwentts.cpp` as a pinned Git submodule and asks Nix to include submodules in its flake source. The submodule intentionally remains an upstream source checkout and its pinned commit does not contain a `flake.nix`. The separate sibling qwentts checkout demonstrates the CMake/OpenBLAS build requirements, but Sophon currently exposes only Rust transcription packages.

## Goals / Non-Goals

**Goals:**
- Expose reproducible CPU, CUDA, and MIGraphX/Vulkan qwentts library packages from Sophon's Linux flake.
- Build exactly the qwentts source pinned by Sophon's submodule.
- Build only the shared-library target and install its runtime libraries and public C header.
- Validate CPU and MIGraphX/Vulkan package artifacts without requiring model downloads, audio inference, or GPU hardware.

**Non-Goals:**
- Adding a flake or any Nix-managed file to `third_party/qwentts.cpp`.
- Adding qwentts TTS support to Sophon's Rust service or D-Bus API.
- Shipping qwentts in the existing Sophon CPU/CUDA/ROCm daemon packages.
- Building qwentts command-line tools, running qwentts inference, or shipping model data.

## Decisions

### Define the qwentts derivation in Sophon's flake

`flake.nix` will contain a shared qwentts derivation factory whose source is the qwentts subdirectory of Sophon's flake source. It will build only the upstream `qwen` CMake target, with a shared library, OpenBLAS, and CPU enabled for every variant. The CUDA variant additionally enables `GGML_CUDA`; the Sophon-named MIGraphX variant enables `GGML_VULKAN` (qwentts has no native MIGraphX backend). Each output installs `libqwen.so`, its required ggml backend libraries, and `qwen.h`, with no qwentts command-line tools.

This keeps the upstream checkout byte-for-byte unmodified and makes the submodule SHA the sole source-version pin. A path flake input is not appropriate because the pinned submodule is source-only and contains no flake entry point. An external qwentts flake input would create a second independently pinned source and could diverge from the submodule.

### Share Sophon's nixpkgs package set

The qwentts derivation will use the same system-specific `pkgs` set as Sophon's existing packages. This avoids a second nixpkgs input and prevents needless incompatibilities or closure duplication between qwentts' native dependencies and the rest of the flake.

### Publish backend-specific qwentts libraries separately

The CPU, CUDA, and MIGraphX/Vulkan builds will be exposed as named qwentts package outputs, separate from the existing Sophon service variants and default package. Consumers can install or depend on them directly; the existing service packages remain transcription-only and do not acquire qwentts closures.

### Validate installed artifacts rather than inference

Flake checks will depend on the CPU and MIGraphX/Vulkan qwentts packages and assert that their shared libraries and public header exist. CUDA will be evaluated but not built by the check suite. This checks packaging and installation deterministically without downloading models or requiring hardware.

## Risks / Trade-offs

- [Submodule omitted from a source checkout] → Keep `inputs.self.submodules = true`, retain the submodule declaration, and make package evaluation/build fail clearly if the expected source is absent.
- [Upstream CMake output names or install expectations change] → Pin the submodule revision and validate the installed artifact set in a flake check; update the packaging definition intentionally when advancing it.
- [Git metadata is absent in Nix's source snapshot] → The upstream version-header generator already falls back to an `unknown` version; do not make Git history a build requirement.
- [Accelerator toolchain cost] → Evaluate CUDA without building it in flake checks; validate CPU and MIGraphX/Vulkan packages only.
