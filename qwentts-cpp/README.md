# qwentts-cpp

`qwentts-cpp` is a safe Rust wrapper around the vendored qwentts.cpp `qwen.h`
C ABI. It owns native contexts, audio buffers, and voice-reference buffers, so
callers do not need unsafe Rust.

## Native build configuration

Cargo builds the pinned `third_party/qwentts.cpp` `qwen` shared library and its
ggml dependencies in the Cargo target directory. Every build requires CMake, a
C/C++ compiler, pkg-config, libclang (for bindgen), and OpenBLAS development
headers and libraries. Initialize the repository submodules before building.

Exactly one acceleration feature must be enabled:

| Feature | Default | Additional prerequisites |
| --- | --- | --- |
| `cpu` | yes | none beyond the common prerequisites |
| `cuda` | no | CUDA toolkit, including `nvcc` and cuBLAS |
| `sycl` | no | Intel-compatible SYCL compiler/SDK with `-fsycl` support |
| `vulkan` | no | Vulkan headers and loader plus `glslc` or `glslangValidator` |

Build the default CPU backend or select one accelerator by disabling defaults:

```sh
cargo test -p qwentts-cpp
cargo build -p qwentts-cpp --no-default-features --features cuda
cargo build -p qwentts-cpp --no-default-features --features sycl
cargo build -p qwentts-cpp --no-default-features --features vulkan
```

Enabling zero backends or combining backends (including adding an accelerator
without `--no-default-features`) fails before native compilation and names the
conflicting selection. Accelerator configuration failures identify the selected
feature and its missing compiler or SDK.

The former `QWENTTS_INCLUDE_DIR` and `QWENTTS_LIB_DIR` variables are no longer
part of the build contract and are ignored. Bindings are generated privately
from the vendored header, and executables/tests receive runtime search metadata
for the native libraries built in Cargo's output directory.

## Lifecycle

Create an engine, load its paired talker and codec models, synthesize, then
unload or drop it:

```no_run
use qwentts_cpp::QwenTtsEngine;

let mut engine = QwenTtsEngine::new();
engine.load_model("talker.gguf", "codec.gguf")?;
let audio = engine.synthesize("Hello!", None)?;
audio.write_wav("hello.wav")?;
# Ok::<(), qwentts_cpp::QwenTtsError>(())
```

`QwenTtsEngine` is intentionally mutable and starts unloaded. Synthesis before
loading returns `QwenTtsError::ModelNotLoaded`. Loading a replacement model
releases the old native context, and `Drop` releases the final context.
`SynthesisResult` owns its f32 samples and remains usable after the engine is
unloaded or dropped.

`SynthesisOptions` supports language and sampling settings plus `Voice` modes:
default, a named CustomVoice speaker, a clone `VoiceReference`, a clone with a
transcript, and a VoiceDesign instruction. Extract clone references from mono
24 kHz f32 PCM with `extract_voice_reference`; they automatically release their
native storage when dropped.

This initial wrapper deliberately has no streaming API and no provider-neutral
abstraction. Higher-level Sophon integration belongs above this Qwen-specific
crate.
