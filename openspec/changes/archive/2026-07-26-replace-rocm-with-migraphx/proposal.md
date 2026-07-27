## Why

Sophon exposes a deferred ROCm accelerator that cannot be packaged from the required ONNX Runtime release archives. The bundled `transcribe-rs` fork and nixpkgs now provide a reproducible path to ORT's MIGraphX execution provider, so Sophon can offer AMD GPU acceleration instead of retaining an unusable ROCm TODO.

## What Changes

- Add a MIGraphX-accelerated ONNX Runtime variant, enabled only by an explicit Cargo feature and delivered as the `sophon-migraphx` Nix package.
- Replace the public accelerator name `rocm` with `migraphx` in configuration, validation, backend provider selection, package names, checks, and documentation.
- **BREAKING** Reject `accelerator: rocm`; it is obsolete and is not retained as an alias.
- Replace downloaded ONNX Runtime release archives with the nixpkgs-provided `onnxruntime` package; enable its `rocmSupport` option only for the MIGraphX package because that is the nixpkgs configuration name.
- Use the local `third_party/transcribe-rs-migraphx` fork to register ORT's MIGraphX execution provider.

## Capabilities

### New Capabilities
- `migraphx-onnx-acceleration`: MIGraphX configuration, provider selection, and reproducible Nix packaging for AMD-accelerated ONNX transcription.

### Modified Capabilities
- None. The existing ROCm behavior is documented only in the still-active v1 change rather than an archived baseline specification; this change captures its replacement as a new capability.

## Impact

Affected areas include `Cargo.toml` and the lockfile, the `transcribe-rs` dependency source, accelerator configuration and backend setup, the Nix flake and closure/provider checks, documentation, and the existing ROCm known-issue record. Existing YAML configurations using `accelerator: rocm` must be changed to `accelerator: migraphx` before upgrading.
