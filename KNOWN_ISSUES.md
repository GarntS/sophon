# Known issues

## NixOS MIGraphX hardware and model compatibility

The nixpkgs `onnxruntime` package exposes the ability to build it with support for CPU-only, CUDA, or "ROCm". In recent builds the ROCm target actually builds with support for MIGraphX, a wrapper relying upon ROCm, instead of the true ROCm backend. As Sophon relies on the `transcribe-rs` crate, which doesn't have upstream support for `ORT`'s MIGraphX execution provider, we have to maintain our own patched fork of `transcribe-rs` for the time being. A patch has been submitted upstream to `transcribe-rs` that would add this functionality. Once merged and released, we can remove the fork.

To try and disambiguate, Sophon packages ONNX Runtime's MIGraphX execution provider on Nix as `sophon-migraphx`. ONNX Runtime's ROCm and MIGraphX providers are distinct, so Sophon also deliberately rejects `accelerator: rocm` instead of treating it as an alias.
