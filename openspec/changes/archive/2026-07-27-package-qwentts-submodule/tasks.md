## 1. qwentts backend library packages

- [x] 1.1 Replace the CPU-only qwentts derivation with a shared derivation factory that builds only the upstream `qwen` library target from `third_party/qwentts.cpp` using Sophon's system-specific nixpkgs package set.
- [x] 1.2 Expose CPU, CUDA, and Sophon-named MIGraphX/Vulkan qwentts library package outputs with CPU; CPU+CUDA; and CPU+Vulkan backends respectively, without qwentts command-line tools.
- [x] 1.3 Install `libqwen.so`, required ggml runtime libraries, and `qwen.h` with valid runtime paths for every variant.

## 2. Packaging validation

- [x] 2.1 Add deterministic CPU and MIGraphX/Vulkan flake checks that verify library artifacts without model downloads or inference, while leaving CUDA evaluation-only.
- [x] 2.2 Build the CPU and MIGraphX/Vulkan qwentts packages and run the targeted qwentts checks, resolving packaging or artifact-layout failures without running the full CUDA flake check.
