## Context

Qwen Base, CustomVoice, and VoiceDesign providers expose distinct public types because each carries different defaults and invariants. Internally, however, `QwenProviderEngine` expands four voice choices into eight execution methods (`synthesize_*` and `stream_*`). Both the native adapter and fixture reproduce that matrix. Each public provider also duplicates request preparation between its buffered and streaming methods.

The private test seam is useful and must remain, but its shape should follow the actual operation: execute one prepared voice invocation either buffered or streaming. Existing public Qwen APIs, feature gates, mode semantics, native behavior, errors, and tests are constraints.

## Goals / Non-Goals

**Goals:**

- Represent Qwen voice selection with one private borrowed enum.
- Give the private engine seam one buffered and one streaming operation plus speaker discovery.
- Prepare each provider request in one mode-specific helper shared by its buffered and streaming paths.
- Isolate Qwen-specific implementation and tests while preserving all root `sophon::tts` exports.
- Preserve current validation precedence, error variants/messages, and recovery behavior.

**Non-Goals:**

- No unification of the three public provider types into one runtime-mode type.
- No changes to `TtsProvider`, public Qwen constructor/method signatures, `TtsRequest`, `VoiceIntent`, or capabilities.
- No sampling, language mapping, model registry, native bridge, queue, stream buffering, or playback changes.
- No new dependency or generic provider framework.

## Decisions

1. **Use one borrowed private voice enum.** Add a private representation equivalent to:

   ```rust
   enum QwenVoiceRequest<'a> {
       Default,
       Named(&'a str),
       Design(&'a str),
       Clone { samples_24khz_mono: &'a [f32], transcript: Option<&'a str> },
   }
   ```

   Borrowing avoids cloning text, descriptions, speaker names, transcripts, or reference samples. It expresses the current native operation without merging public provider modes.

2. **Collapse the private engine execution surface.** Replace the eight mode-specific execution methods on `QwenProviderEngine` with `synthesize(text, language, voice)` and `synthesize_streaming(text, language, voice, emit)`, retaining `speakers()`. `QwenEngineAdapter` maps the private voice enum to the existing native `QwenVoice`; clone variants still extract a temporary native reference before execution. Existing public methods on `QwenEngineAdapter` remain as compatibility wrappers and retain their signatures.

3. **Prepare once per provider mode.** Add one private preparation helper for each of Base, CustomVoice, and VoiceDesign. Both `TtsProvider::synthesize` and `synthesize_streaming` call that helper and differ only in which engine execution method they invoke. Each helper SHALL preserve its current validation order:

   - CustomVoice: capabilities and voice membership, synthesis text limit/controls, unit speed, language normalization, then default/named speaker selection.
   - VoiceDesign: capabilities, unit speed, synthesis text limit/controls, language normalization, default/request description selection, then description limit/controls.
   - Base: capabilities, unit speed, synthesis text limit/controls, language normalization, then clone transcript and 24 kHz reference validation when applicable.

   This retains externally visible error precedence while eliminating buffered/streaming drift.

4. **Keep public provider wrappers because they encode invariants.** The three provider structs remain separate and public. Shared dispatch does not move mode defaults into a loosely typed configuration or permit unsupported intents. This avoids trading duplication for weaker state modeling.

5. **Create `src/tts/qwen.rs` with compatibility re-exports.** Move Qwen language/text helpers, adapter, private seam, three public providers, and Qwen-focused tests under the existing Qwen feature condition. `src/tts/mod.rs` re-exports `normalize_qwen_language`, `QwenEngineAdapter`, and the three provider types at their current paths. `create_tts_provider` may remain in `mod.rs` and consume the re-exports, or move only if that does not broaden the Qwen module's responsibilities.

## Risks / Trade-offs

- **[Risk] Borrowed invocation lifetimes conflict with mutable engine access.** → Keep preparation helpers free/static where necessary and split borrows by provider fields; do not clone large reference audio as a workaround.
- **[Risk] Consolidation changes which validation error wins.** → Preserve the provider-specific order above and add buffered/streaming parity tests for invalid inputs.
- **[Risk] Public exports or feature-gated builds regress after file extraction.** → Re-export exact names and run no-default/CPU, CUDA, and Vulkan compile checks.
- **[Trade-off] One private enum is introduced.** → It replaces eight execution methods and directly represents the existing voice operation.
- **[Risk] Native and fixture paths diverge.** → Drive both through the same private trait shape and table-test every voice variant through both execution modes.

## Migration Plan

1. Apply `isolate-tts-worker-module` first when following the recommended sequence.
2. Add the private invocation representation and collapse the private engine trait/native fixture implementations.
3. Add one request-preparation helper per public Qwen provider and route buffered/streaming methods through it.
4. Move Qwen implementation and tests to `src/tts/qwen.rs`; add compatibility re-exports.
5. Run all Qwen unit tests, feature compile checks, workspace validation, CPU Nix runtime check, and optionally the ignored real-model smoke test when curated models are available.

Rollback restores the prior private trait and source location; no data or rollout migration is required.
