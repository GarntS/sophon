## 1. Model one private Qwen invocation

- [x] 1.1 Add the borrowed private default/named/design/clone invocation enum without cloning text, descriptions, speaker names, transcripts, or reference samples.
- [x] 1.2 Replace the eight mode-specific `QwenProviderEngine` execution methods with one buffered and one streaming method plus `speakers`, and update the native adapter and fixture implementation.
- [x] 1.3 Preserve existing public `QwenEngineAdapter` methods as compatibility wrappers over the simplified private dispatch.

## 2. Consolidate provider request preparation

- [x] 2.1 Add one Base preparation helper shared by buffered and streaming execution, preserving capability, speed, text, language, transcript, and 24 kHz reference validation order.
- [x] 2.2 Add one CustomVoice preparation helper shared by buffered and streaming execution, preserving voice membership, text, speed, language, and default-speaker behavior.
- [x] 2.3 Add one VoiceDesign preparation helper shared by buffered and streaming execution, preserving speed, text, language, default/override description, and description validation behavior.
- [x] 2.4 Route each provider's buffered and streaming methods through the same prepared invocation while retaining the three separate public provider types and capability sets.

## 3. Isolate Qwen provider code

- [x] 3.1 Move Qwen helpers, adapter, private engine seam, public Qwen providers, and Qwen-focused unit tests into `src/tts/qwen.rs` under the existing feature condition.
- [x] 3.2 Re-export `normalize_qwen_language`, `QwenEngineAdapter`, `QwenTtsBaseProvider`, `QwenTtsCustomVoiceProvider`, and `QwenTtsVoiceDesignProvider` from `sophon::tts` with unchanged signatures.
- [x] 3.3 Keep `create_tts_provider`, `src/main.rs`, and `tests/qwen_real_model_smoke.rs` using their current public paths and behavior.

## 4. Verify mode and execution parity

- [x] 4.1 Add table-driven buffered/streaming tests covering every supported default, named, design, and clone invocation, including configured defaults and request overrides.
- [x] 4.2 Add parity tests showing invalid capability, speed, text, language, transcript, description, voice, and reference inputs produce the same error classification before native work in both execution modes.
- [x] 4.3 Verify a native fixture failure remains request-local and a subsequent request succeeds.

## 5. Run project validation

- [x] 5.1 Run `nix develop -c cargo fmt --all -- --check`.
- [x] 5.2 Run `nix develop -c cargo clippy --all-targets -- -D warnings`.
- [x] 5.3 Run `nix develop -c cargo test --workspace`.
- [x] 5.4 Run `nix build .#checks.x86_64-linux.sophon-cpu-qwen-runtime`.
- [x] 5.5 CUDA and MIGraphX Qwen runtime checks not run: this builder has no NVIDIA tool/device and ROCm reports only a CPU agent; retain both as required CI validation on backend-capable builders.
- [x] 5.6 Curated Qwen GGUF fixtures are unavailable; retain `nix develop -c cargo test --test qwen_real_model_smoke -- --ignored --nocapture` as explicit opt-in validation.
