## 1. Replace the ROCm accelerator contract

- [x] 1.1 Replace the `rocm` Cargo feature with a feature that enables MIGraphX in both the local `transcribe-rs` fork and `ort`.
- [x] 1.2 Change the accelerator configuration and backend mapping to select `OrtAccelerator::Migraphx`, and add tests that an explicit MIGraphX request is accepted only in a MIGraphX build.
- [x] 1.3 Add a strict configuration test proving `accelerator: rocm` is rejected as obsolete without an alias.
- [x] 1.4 Update Cargo.lock for the local `transcribe-rs` path dependency and verify CPU, CUDA, and MIGraphX feature builds do not activate unintended providers.

## 2. Package nixpkgs ONNX Runtime variants

- [x] 2.1 Replace custom fetched ONNX Runtime release derivations with nixpkgs `onnxruntime` variants while retaining dynamic linking, offline ORT builds, and binary RPATH configuration.
- [x] 2.2 Add `sophon-migraphx`, built with Sophon's MIGraphX feature and `onnxruntime.override { rocmSupport = true; }`, and remove all deferred `sophon-rocm` package paths.
- [x] 2.3 Extend provider smoke coverage to verify the MIGraphX package registers its provider without physical GPU inference.
- [x] 2.4 Extend closure-policy and evaluation checks to cover the MIGraphX package and preserve CPU/CUDA exclusion of ROCm/MIGraphX dependencies.

## 3. Document and validate the migration

- [x] 3.1 Replace ROCm references in the README, sample configuration, and known-issues record with MIGraphX guidance, including the breaking YAML migration.
- [x] 3.2 Run formatting, clippy, Rust tests, and Nix builds/checks for CPU, CUDA, and MIGraphX variants; record any nixpkgs ONNX Runtime compatibility issue as a blocker.
