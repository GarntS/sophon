## Context

Sophon currently uses crates.io `transcribe-rs` 0.3.11 and downloaded ONNX Runtime 1.24.4 CPU/CUDA archives. Its `rocm` Cargo feature, `Accelerator::Rocm` configuration variant, and ROCm documentation are placeholders because no compatible Linux ROCm archive was available. The local `third_party/transcribe-rs-migraphx` fork contains one additional commit that exposes ORT's MIGraphX provider. Current nixpkgs provides ONNX Runtime 1.26.0 and can build it with the MIGraphX provider through `rocmSupport = true`.

## Goals / Non-Goals

**Goals:**

- Deliver a separately installable `sophon-migraphx` package with AMD GPU acceleration and CPU fallback.
- Use `migraphx` consistently in Sophon's public interfaces and reject the obsolete `rocm` configuration value.
- Build every Sophon package against nixpkgs-provided ONNX Runtime without Cargo or Nix build steps downloading ORT release archives.
- Preserve the CPU package's minimal closure and the CUDA package's CUDA-only provider closure.

**Non-Goals:**

- Support ORT's ROCm execution provider, retain a ROCm compatibility alias, or ship a `sophon-rocm` package.
- Change model formats, D-Bus methods, model acquisition, or runtime provider selection after a model session is created.
- Support non-NixOS packaging or guarantee MIGraphX support for every AMD GPU/model combination.

## Decisions

### Replace ROCm with MIGraphX as the Sophon accelerator contract

The YAML enum, Rust feature, `OrtAccelerator` selection, package name, provider smoke test, closure policy, README, and known-issues documentation will use `migraphx`. `rocm` will be removed from deserialization, so strict configuration validation rejects it rather than silently mapping users to a different execution provider.

This is intentionally breaking: ORT's ROCm and MIGraphX execution providers are distinct. Retaining an alias would misrepresent what the installed package actually runs.

### Use the local transcribe-rs MIGraphX fork as a path dependency

Sophon will depend on `third_party/transcribe-rs-migraphx` rather than crates.io for `transcribe-rs`. Its `ort-migraphx` feature will be enabled only by Sophon's `migraphx` feature, alongside `ort/migraphx`. The CPU and CUDA builds must not enable this feature.

This reuses the maintained narrow fork while upstream support is unavailable to the pinned release. Alternatives considered: patching transcribe-rs within Sophon duplicates the fork; exposing ORT directly from Sophon bypasses the library's global accelerator/session setup.

### Source ONNX Runtime from nixpkgs for all variants

The flake will replace custom downloaded runtime derivations with `pkgs.onnxruntime` for CPU and an appropriate nixpkgs configuration for CUDA. The MIGraphX runtime will be `pkgs.onnxruntime.override { rocmSupport = true; }`; `rocmSupport` is used only because it is nixpkgs' build-option name and maps to `onnxruntime_USE_MIGRAPHX`.

Each Sophon build continues to set the ORT dynamic-link environment to its selected runtime and adds that runtime to the installed binary's RPATH. The package-specific runtime prevents a CPU or CUDA installation from carrying the MIGraphX/ROCm closure.

### Validate package/provider behavior without physical AMD hardware

The flake will evaluate/build `sophon-migraphx`, run a provider-registration smoke program built with the MIGraphX feature, and assert package closure separation. Provider registration validates that ONNX Runtime can load the compiled provider; full inference remains an opt-in hardware/model smoke test.

## Risks / Trade-offs

- **[ORT binding/runtime version gap]** `ort` is pinned for ONNX Runtime 1.24 while nixpkgs currently supplies 1.26. → Build and run provider smoke checks against nixpkgs; pin the flake lock and investigate any API/ABI failure before release.
- **[Large and expensive closure]** The MIGraphX runtime pulls ROCm build/runtime dependencies. → Keep it in a separate package and assert CPU/CUDA closure exclusions.
- **[MIGraphX model or GPU limitations]** A provider can register but decline parts of a model graph or lack support for a GPU. → CPU remains the final provider fallback; document hardware/model validation as operational testing.
- **[Fork divergence]** The local dependency can fall behind upstream. → Keep the fork's change narrowly scoped and replace it with an upstream release when available.

## Migration Plan

1. Users replace `accelerator: rocm` with `accelerator: migraphx` and install `.#sophon-migraphx`.
2. The old ROCm feature and all `sophon-rocm` references are removed; an old configuration fails startup validation with an actionable invalid-accelerator error.
3. Rollback consists of installing `sophon-cpu` or `sophon-cuda` and selecting `cpu`, `cuda`, or `auto`; no model-cache migration is required.
