## Why

The private Qwen provider seam models four voice modes as eight buffered/streaming methods, and the three public providers repeat the same request preparation in six execution methods. A single invocation model can express the real operation directly while retaining all public APIs and mode-specific invariants.

## What Changes

- Add one private borrowed Qwen invocation representation for default, named, design, and clone voices.
- Reduce the private `QwenProviderEngine` execution surface from four buffered plus four streaming methods to one buffered and one streaming operation, retaining speaker discovery.
- Prepare and validate each provider request once per provider mode, then route that prepared invocation through either buffered or streaming execution.
- Move Qwen-specific adapter/provider implementation and tests into a focused `src/tts/qwen.rs` module, re-exporting every existing public Qwen type and function from `sophon::tts`.
- Preserve all public signatures, feature gates, validation order and classifications, model-mode behavior, native calls, streaming behavior, and diagnostics.

## Capabilities

### New Capabilities

- `qwen-dispatch-compatibility`: Compatibility requirements for simplifying private Qwen dispatch without changing public or provider-observable behavior.

### Modified Capabilities

None. The existing `qwen-tts-providers` behavior contract is preserved rather than changed.

## Impact

- Current implementation and unit tests: Qwen sections of `src/tts/mod.rs` (`QwenEngineAdapter`, `QwenProviderEngine`, three public Qwen providers, language/text helpers, and Qwen tests).
- New focused location after implementation: `src/tts/qwen.rs`, with compatibility re-exports from `src/tts/mod.rs`.
- Verified consumers: `src/main.rs`, `tests/qwen_real_model_smoke.rs`, `create_tts_provider`, `TtsProvider` worker routing, and Qwen-focused unit tests.
- Public compatibility boundary: `normalize_qwen_language`, `QwenEngineAdapter`, `QwenTtsBaseProvider`, `QwenTtsCustomVoiceProvider`, `QwenTtsVoiceDesignProvider`, and their existing public methods.
- No manifest, configuration, registry, native bridge, D-Bus, data format, or persisted-data changes.
- Ordering: apply `isolate-tts-worker-module` first to reduce overlap; this change remains confined to provider code and can precede `bound-tts-stream-handoff`.
