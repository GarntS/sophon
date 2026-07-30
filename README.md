# Sophon

Sophon is a headless session D-Bus speech-to-text (STT) and text-to-speech (TTS) service. It performs local inference and exchanges complete audio through files, transferred descriptors, or PipeWire playback.

STT uses [`transcribe-rs`](https://github.com/cjpais/transcribe-rs.git) with Parakeet or Canary on CPU, CUDA, or AMD MIGraphX ONNX Runtime packages. TTS produces mono 24 kHz float PCM using either Kokoro from `tts-rs` or curated Qwen3-TTS Base, CustomVoice, and VoiceDesign models through `qwentts-cpp`.

## Install

This project targets NixOS and provides backend-paired packages:

```sh
nix profile install .#sophon-cpu       # ONNX CPU + Qwen CPU/OpenBLAS
nix profile install .#sophon-cuda      # ONNX CUDA + Qwen CUDA
nix profile install .#sophon-migraphx  # ONNX MIGraphX + Qwen Vulkan
```

| Package | STT backend | Qwen backend | Runtime requirements |
|---|---|---|---|
| `sophon-cpu` | ONNX Runtime CPU | GGML CPU/OpenBLAS | x86-64 CPU and sufficient RAM |
| `sophon-cuda` | ONNX Runtime CUDA | GGML CUDA | compatible NVIDIA driver/CUDA device |
| `sophon-migraphx` | ONNX Runtime MIGraphX | GGML Vulkan | compatible AMD ROCm/MIGraphX stack plus a Vulkan loader and device |

Each output includes `libqwen`, common GGML libraries, its selected GGML backend, and relocatable runtime search paths. Accelerator packages retain CPU fallback libraries but do not include the unrelated Qwen accelerator. The runtime closure also includes PipeWire and `espeak-ng`, which Kokoro uses for phonemization.

### Validation

Ordinary checks are model-free: they compile the ignored Qwen smoke harness but never download or load multi-gigabyte GGUF files.

```sh
nix develop -c cargo fmt --all -- --check
nix develop -c cargo clippy --all-targets -- -D warnings
nix develop -c cargo test --workspace
nix build .#checks.x86_64-linux.sophon-cpu-qwen-runtime
nix build .#checks.x86_64-linux.dbus-activation
```

Backend-capable builders can additionally run `sophon-cuda-qwen-runtime`, `sophon-migraphx-qwen-runtime`, and the full `nix flake check`. Those checks inspect installed native libraries, loader resolution, exact backend selection, RPATHs, and accelerator closure policy.

Real-model synthesis is explicitly opt-in. Supply exact curated files matching the selected registry manifests, then run the ignored harness:

```sh
export SOPHON_QWEN_CODEC=/models/qwen-tokenizer-12hz-Q8_0.gguf
export SOPHON_QWEN_BASE_TALKER=/models/qwen-talker-0.6b-base-Q8_0.gguf
export SOPHON_QWEN_CUSTOM_VOICE_TALKER=/models/qwen-talker-0.6b-customvoice-Q8_0.gguf
export SOPHON_QWEN_VOICE_DESIGN_TALKER=/models/qwen-talker-1.7b-voicedesign-Q8_0.gguf
nix develop -c cargo test --test qwen_real_model_smoke -- --ignored --nocapture
```

The heavyweight harness verifies finite, nonempty mono 24 kHz output for default, named, one-shot clone, and voice-design synthesis.

## Configuration

Sophon reads `$XDG_CONFIG_HOME/sophon/config.yaml` once at startup. Changes require a daemon restart. Unknown fields and malformed values are rejected rather than ignored.

```yaml
# STT
provider: transcribe-rs
model_id: parakeet-tdt-0.6b-v3-int8
accelerator: auto # auto | cpu | cuda | migraphx
language: en
# cache_dir: /absolute/shared-model-cache
max_audio_bytes: 33554432
max_audio_seconds: 600
queue_capacity: 8
log_level: info

# TTS (all fields optional)
tts:
  provider: tts-rs
  model_id: kokoro-v1.0-int8
  default_voice: af_heart
  default_speed: 1.0 # finite, 0.5 through 2.0
  # pipewire_node: alsa_output.example # exact stable node.name
  volume: 1.0 # finite linear gain, 0.0 through 1.0
  max_text_bytes: 16384
  max_reference_audio_bytes: 33554432
  max_reference_audio_seconds: 60
  max_generated_audio_seconds: 600
  queue_capacity: 8
```

STT and TTS share `$XDG_CACHE_HOME/sophon/models` unless the top-level `cache_dir` overrides it. The package's read-only `model_registry.yaml` is the only model catalog. Missing or invalid files are always downloaded from its pinned HTTPS URLs; local model paths, provider-specific caches, and download-policy switches are not supported. Artifacts live at `artifacts/<sha256>` and model views contain hard links, so models sharing bytes use one verified blob. Download progress is aggregate verified/downloaded bytes rather than completed-file count. A model's first resolution failure is terminal for that daemon process; restart Sophon to retry. Independently completed blobs remain reusable.

The Kokoro int8 model is approximately 88 MiB and its voice archive approximately 27 MiB, for an initial download/cache footprint of roughly 115 MiB, excluding the generated optimized ONNX graph.

### Qwen3-TTS models

All Qwen files are Q8_0 GGUF artifacts pinned to revision `e0f336a048a3de02b29b8ad92969217d9ecffe3e` of `Serveurperso/Qwen3-TTS-GGUF`. Every model uses the same `qwen-tokenizer-12hz-Q8_0.gguf` codec (291,150,624 bytes, about 278 MiB), which is stored once in the content-addressed cache.

| Model ID | Mode | Talker size | Pair footprint | Default |
|---|---|---:|---:|---|
| `qwen3-tts-0.6b-base-q8_0` | Base | 992,615,488 B | ~1.20 GiB | Base |
| `qwen3-tts-1.7b-base-q8_0` | Base | 2,079,448,256 B | ~2.21 GiB | |
| `qwen3-tts-0.6b-custom-voice-q8_0` | CustomVoice | 968,588,544 B | ~1.17 GiB | CustomVoice |
| `qwen3-tts-1.7b-custom-voice-q8_0` | CustomVoice | 2,042,834,304 B | ~2.17 GiB | |
| `qwen3-tts-1.7b-voice-design-q8_0` | VoiceDesign | 2,042,833,824 B | ~2.17 GiB | VoiceDesign |

Qwen configuration is typed by the selected model. Fields belonging to another mode are rejected:

```yaml
# Base: default synthesis and one-shot cloning
tts:
  provider: qwentts-cpp
  model_id: qwen3-tts-0.6b-base-q8_0 # provider-only config also defaults here
  # default_clone_reference: /absolute/reference-24khz-mono-f32.wav
  # default_clone_transcript: Optional words spoken in the reference
  default_speed: 1.0
  sampling:
    # seed: 42 # omit for random seeds; configure for deterministic replay
    max_new_tokens: 2048
    temperature: 0.9
    top_k: 50
    top_p: 1.0
    repetition_penalty: 1.05
```

```yaml
# CustomVoice: named speakers
tts:
  provider: qwentts-cpp
  model_id: qwen3-tts-0.6b-custom-voice-q8_0
  default_voice: vivian
```

```yaml
# VoiceDesign: a request voice_description can override this for one call
tts:
  provider: qwentts-cpp
  model_id: qwen3-tts-1.7b-voice-design-q8_0
  default_voice_description: A warm, clear, natural adult voice with moderate pitch and pace.
```

The daemon-wide Qwen sampling policy cannot be overridden per request. Defaults are a random seed, 2048 new tokens, temperature 0.9, top-k 50, top-p 1.0, and repetition penalty 1.05. The effective token maximum is the lower of `max_new_tokens` and the native conversion of `max_generated_audio_seconds`. Configured numeric seeds are reused for deterministic requests. Request-level clone audio/transcript, named voice, or voice description overrides the corresponding startup default for that request only.

### Breaking configuration migration

Replace STT `engine` with `provider: transcribe-rs`. Remove `quantization`, `translate`, every `model_path`, every `automatic_download`, and `tts.cache_dir`; these fields now fail strict parsing. Keep only the top-level shared `cache_dir`. Choose provider/model pairs present in the installed package registry. Existing content-addressed blobs can be reused, but old model-specific layouts are rebuilt as registry views.

Omitted Qwen language selects automatic detection. Supported case-insensitive base and documented regional tags cover English, Chinese, Japanese, Korean, German, French, Russian, Portuguese, Spanish, and Italian; unsupported tags return `InvalidTtsOptions` instead of falling back to English.

| Provider mode | Named voices | One-shot cloning | Voice design | Speed control |
|---|---:|---:|---:|---:|
| Kokoro | yes | no | no | yes (0.5–2.0) |
| Qwen Base | no | yes | no | no (must be 1.0) |
| Qwen CustomVoice | yes | no | no | no (must be 1.0) |
| Qwen VoiceDesign | no | no | yes | no (must be 1.0) |

TTS configuration failure is isolated from STT initialization, and STT failure does not overwrite TTS lifecycle state.

## D-Bus API

Name: `com.garntresearch.sophon`  
Path: `/com/garntresearch/sophon`
Interface: `com.garntresearch.sophon`

### Methods

- `TranscribeFile(s path, a{sv} options) -> s`
- `TranscribeMemfd(h fd, a{sv} options) -> s`
- `SpeakToFile(s text, s path, a{sv} options) -> t size_bytes`
- `SpeakToBuffer(s text, a{sv} options) -> (h fd, t size_bytes)`
- `SpeakAloud(s text, a{sv} options) -> ()`

Transcription options recognize only `language` as a string. Translation is not supported; clients should use a separate translation service.

`SpeakToFile` requires an absolute path that does not exist. Creation is exclusive, concurrent creation returns `OutputExists`, and a failed write removes only the partial file Sophon created.

`SpeakToBuffer` returns a server-created memfd positioned at byte zero. Its complete WAV contents and size are immutable using Linux write, grow, shrink, and further-sealing seals. The transferred client descriptor remains readable after the server drops its reference.

`SpeakAloud` returns only after complete playback drains or fails. Calls are serialized and never overlap.

### TTS options

Options are strict D-Bus variants:

| Key | Type | Meaning |
|---|---|---|
| `voice` | `s` | Named voice advertised by `AvailableVoices` |
| `language` | `s` | Language tag compatible with the selected voice |
| `speed` | `d` | Finite multiplier from `0.5` through `2.0` for providers advertising `speed-control`; Qwen requires `1.0` |
| `clone_audio` | `h` | Transferred canonical reference-WAV descriptor |
| `clone_transcript` | `s` | Optional transcript; requires `clone_audio` |
| `voice_description` | `s` | Provider-specific voice-design intent |

`voice`, `clone_audio`, and `voice_description` are mutually exclusive. Omitted voice and speed use configured defaults. Unknown keys, wrong variant types, unavailable voices, contradictory intents, orphan clone transcripts, invalid language/voice combinations, and invalid speed return `InvalidTtsOptions` before inference is queued.

Kokoro supports default and named voices. Qwen Base supports default synthesis and cloning, CustomVoice supports default/named speakers, and VoiceDesign supports its configured default plus per-request design descriptions. Unsupported valid intents return `UnsupportedCapability` without fallback. `TtsCapabilities` reports `named-voices`, `voice-cloning`, `voice-design`, and `speed-control` as applicable.

### Lifecycle and capability discovery

STT properties are `State`, `ActiveProvider`, `ActiveModel`, `DownloadProgress`, and `LastError`.

Independent TTS properties are:

- `TtsState`
- `ActiveTtsProvider`
- `ActiveTtsModel`
- `TtsDownloadProgress`
- `TtsLastError`
- `AvailableVoices`
- `TtsCapabilities`

States progress through `Initializing`, `Downloading`, `Loading`, `Ready`, and `Failed`. Standard `PropertiesChanged` signals report updates. Clients should inspect `AvailableVoices` and `TtsCapabilities` instead of assuming provider support.

### Stable errors

Errors use the `com.garntresearch.sophon` namespace:

- Shared: `NotReady`, `ModelUnavailable`, `ResourceLimit`
- STT: `InvalidOptions`, `InvalidAudio`, `TranscriptionFailed`
- TTS: `InvalidTtsOptions`, `InvalidReferenceAudio`, `UnsupportedCapability`, `OutputExists`, `OutputFailed`, `SynthesisFailed`, `PlaybackFailed`

Calls during initialization return retryable `NotReady`; calls after the relevant model initialization fails return `ModelUnavailable`. Provider and playback failures do not stop later queued requests or change the other service's readiness.

Clients should use a timeout appropriate for local model inference plus complete playback. A D-Bus timeout does not cancel accepted synthesis, transcription, or playback work.

## Audio formats and limits

### STT input

STT accepts complete RIFF/WAVE data containing mono 16 kHz signed 16-bit PCM. File paths must be absolute regular files. Transferred descriptors must be readable and seekable from byte zero; they need not be memfds.

### TTS output

File and buffer synthesis returns complete mono RIFF/WAVE with the provider sample rate and 32-bit IEEE-float PCM. Kokoro output is mono 24 kHz float WAV.

### Clone reference input

Clone descriptors must be readable and seekable from byte zero and contain complete mono 24 kHz 32-bit IEEE-float WAV data. Sophon does not resample or remix references. Encoded-byte and decoded-duration limits are checked before synthesis. The initial Kokoro provider rejects otherwise valid cloning as unsupported.

Text bytes, reference bytes/duration, generated duration, inference queue depth, and playback queue depth are bounded by configuration. A full queue or exceeded bound returns `ResourceLimit`.

## PipeWire playback

Without `tts.pipewire_node`, Sophon asks PipeWire for its current default audio sink. When a node is configured, Sophon resolves that exact stable `node.name`; a missing node returns `PlaybackFailed` and never falls back to another sink. Configured volume is a linear multiplier applied only to playback, not file or memfd output. `0.0` performs normal silent playback.

A controlled development smoke harness is available inside `nix develop`:

```sh
tests/pipewire-smoke.sh
```

It starts an isolated PipeWire daemon, creates an exact-name null sink, negotiates mono float audio, and waits for complete stream drain.

## Benchmarks

A manually-run, black-box benchmark harness lives in `tests/bench/` (Python, managed by [`uv`](https://docs.astral.sh/uv/)). It attaches to the already-running daemon on the session bus, discovers the active engine, model, TTS provider, and capabilities from the lifecycle properties, waits for readiness, and measures what a real client experiences. It never starts, stops, or reconfigures the daemon.

```sh
uv run --directory tests/bench sophon-bench [--wav-dir DIR] [--jsonl PATH] \
    [--warmup N] [--reps N] [--timeout SECONDS] [--keep-outputs]
```

Each run measures three things, sequentially:

- **IPC/validation baseline**: timed calls the daemon rejects before inference is queued (a nonexistent audio path, an unknown TTS option key). This is the transport floor every other number includes. Each baseline call is verified to fail with the expected error type.
- **STT sweep**: `TranscribeFile` latency over a corpus bucketed around ~1s/3s/10s/30s durations, with per-call real-time factor (RTF = audio duration / call latency).
- **TTS sweep**: the three embedded text lengths (sentence / paragraph / page) through both `SpeakToFile` and `SpeakToBuffer`, with per-call RTF = call latency / generated audio duration. Buffer cells are skipped with a notice when Unix FD passing cannot be negotiated.

The first post-ready inference in each STT/TTS sweep is recorded separately. Every cell then runs `--warmup` calls (default 3) followed by `--reps` measured repetitions (default 15). The report prints n, min, p50, p90, p99, max, mean, standard deviation, and median RTF per cell; first-call and warmup data never appear in measured distributions. `--jsonl` writes a manifest record (platform, CPU model, discovered daemon configuration, timestamp, harness version) followed by one record per measured call. Generated speech files land in a run-scoped directory removed on completion unless `--keep-outputs` is passed; Ctrl-C keeps partial results.

### STT corpus

`--wav-dir` supplies the audio corpus directly. Files must be 16 kHz mono 16-bit PCM WAV (the daemon's own input constraint); violating files are rejected with a message naming the constraint, and accepted files must span at least three duration buckets. Without `--wav-dir`, the harness synthesizes a corpus through the daemon's own TTS, resamples it to 16 kHz mono with `ffmpeg` or `sox` (whichever is on PATH — an optional runtime detection, never a hard dependency), and caches it under `$XDG_CACHE_HOME/sophon/bench-corpus` for reuse. With neither a corpus nor a way to make one, the STT sweep is skipped with an actionable message; TTS results still stand.

### Reading the numbers

All latencies are **end-to-end** and client-measured: bus round trip, queueing, inference, and output. The `baseline/*` rows identify the transport floor (no inference), so inference cost is roughly a cell's latency minus the baseline. Compare medians before tails; treat p90/p99 as promises about bad days. Caveats:

- Run on an idle machine. CPU governor state and background load inflate variance, and the harness makes no attempt to control them.
- Black-box measurement cannot attribute time between queue wait and inference; sequential single-client runs keep queue wait near zero by construction.
- Qwen TTS generation is nondeterministic, so output length (and hence RTF) varies call to call; a configured `seed` makes TTS runs more reproducible.
- Long texts on CPU Qwen are slow (minutes per call); reduce `--reps` for a quicker pass.

## Etymology

A Sophon is fictional technology from *Remembrance of Earth's Past* by Cixin Liu, translated by Ken Liu. In *The Three-Body Problem*, Sophons are used to communicate covertly with human scientists and stall scientific progress on Earth.

## Known issues

### Qwen inference is serialized and non-cancellable (performance blocker)

Qwen TTS uses the daemon's bounded FIFO TTS worker and runs one complete native inference at a time. Running and queued requests cannot currently be cancelled. If a D-Bus caller disconnects or abandons a request, the accepted native inference continues to completion; every later Qwen request remains behind it, creating head-of-line blocking. The configured queue capacity bounds pending work, but it does not bound execution time or stop the request currently occupying the worker.

This is a performance blocker for interactive and multi-client use, especially on CPU and for long generation limits. A future integration should connect request lifetime to qwentts.cpp's cooperative cancellation callback and add safe queue removal. Until then, clients should use conservative text and generated-duration limits, avoid retrying timed-out requests blindly, and account for earlier abandoned work when choosing timeouts.

### NixOS MIGraphX hardware and model compatibility

The nixpkgs `onnxruntime` package exposes the ability to build it with support for CPU-only, CUDA, or "ROCm". In recent builds the ROCm target actually builds with support for MIGraphX, a wrapper relying upon ROCm, instead of the true ROCm backend. As Sophon relies on the `transcribe-rs` crate, which doesn't have upstream support for `ORT`'s MIGraphX execution provider, we have to maintain our own patched fork of `transcribe-rs` for the time being. A patch has been submitted upstream to `transcribe-rs` that would add this functionality. Once merged and released, we can remove the fork.

To try and disambiguate, Sophon packages ONNX Runtime's MIGraphX execution provider on Nix as `sophon-migraphx`. ONNX Runtime's ROCm and MIGraphX providers are distinct, so Sophon also deliberately rejects `accelerator: rocm` instead of treating it as an alias.
