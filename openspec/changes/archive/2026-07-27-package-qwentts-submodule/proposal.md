## Why

Sophon vendors `qwentts.cpp` as a Git submodule but cannot build or distribute it through its Nix flake. Packaging the submodule in Sophon makes the pinned TTS tools reproducibly available without adding Nix-specific files to the upstream source tree.

## What Changes

- Add CPU, CUDA, and MIGraphX/Vulkan `qwentts.cpp` shared-library package outputs to Sophon's Linux flake, built from `third_party/qwentts.cpp`.
- Build only the upstream `qwen` shared-library target with the backend set appropriate to each package; install the shared libraries and public header, but not command-line tools.
- Keep the qwentts submodule source unmodified; Sophon's flake owns the packaging definition.
- Add deterministic Nix checks for the CPU and MIGraphX/Vulkan package artifact layouts without model downloads or inference; CUDA is evaluation-only.

## Capabilities

### New Capabilities

### Modified Capabilities
- `nix-service-packaging`: Expand flake package outputs and validation to include the vendored qwentts CPU package.

## Impact

- Affected file: `flake.nix`.
- Affected dependency/source: `third_party/qwentts.cpp` Git submodule, which must be present in the flake source.
- New runtime outputs: CPU, CUDA, and MIGraphX/Vulkan qwentts shared libraries and `qwen.h`; Sophon's existing transcription daemon binaries, behavior, and package closures remain unchanged.
