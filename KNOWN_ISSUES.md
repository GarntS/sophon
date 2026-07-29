# Known issues

## Qwen inference is serialized and non-cancellable (performance blocker)

Qwen TTS uses the daemon's bounded FIFO TTS worker and runs one complete native inference at a time. Running and queued requests cannot currently be cancelled. If a D-Bus caller disconnects or abandons a request, the accepted native inference continues to completion; every later Qwen request remains behind it, creating head-of-line blocking. The configured queue capacity bounds pending work, but it does not bound execution time or stop the request currently occupying the worker.

This is a performance blocker for interactive and multi-client use, especially on CPU and for long generation limits. A future integration should connect request lifetime to qwentts.cpp's cooperative cancellation callback and add safe queue removal. Until then, clients should use conservative text and generated-duration limits, avoid retrying timed-out requests blindly, and account for earlier abandoned work when choosing timeouts.

## NixOS MIGraphX hardware and model compatibility

The nixpkgs `onnxruntime` package exposes the ability to build it with support for CPU-only, CUDA, or "ROCm". In recent builds the ROCm target actually builds with support for MIGraphX, a wrapper relying upon ROCm, instead of the true ROCm backend. As Sophon relies on the `transcribe-rs` crate, which doesn't have upstream support for `ORT`'s MIGraphX execution provider, we have to maintain our own patched fork of `transcribe-rs` for the time being. A patch has been submitted upstream to `transcribe-rs` that would add this functionality. Once merged and released, we can remove the fork.

To try and disambiguate, Sophon packages ONNX Runtime's MIGraphX execution provider on Nix as `sophon-migraphx`. ONNX Runtime's ROCm and MIGraphX providers are distinct, so Sophon also deliberately rejects `accelerator: rocm` instead of treating it as an alias.
