# Sophon

Sophon is a headless session D-Bus speech-to-text (STT) and text-to-speech (TTS) service. It performs local inference and exchanges complete audio through files, transferred descriptors, or PipeWire playback.

STT uses [`transcribe-rs`](https://github.com/cjpais/transcribe-rs.git) with Parakeet or Canary on CPU, CUDA, or AMD MIGraphX ONNX Runtime packages. TTS uses the Kokoro engine from `tts-rs`, producing mono 24 kHz float PCM with named voices and speed control.

## Install

This project targets NixOS and provides separate ONNX Runtime packages:

```sh
nix profile install .#sophon-cpu       # default, CPU only
nix profile install .#sophon-cuda      # CUDA with CPU fallback
nix profile install .#sophon-migraphx  # AMD MIGraphX with CPU fallback
```

The runtime closure includes PipeWire and `espeak-ng`, which Kokoro uses for phonemization.

## Configuration

Sophon reads `$XDG_CONFIG_HOME/sophon/config.yaml` once at startup. Changes require a daemon restart. Unknown fields and malformed values are rejected rather than ignored.

```yaml
# STT
engine: parakeet # parakeet | canary
model_id: parakeet-tdt-0.6b-v3-int8
# model_path: /absolute/local/stt-model-directory
quantization: int8 # int8 | fp16 | fp32
accelerator: auto # auto | cpu | cuda | migraphx
language: en
translate: false
# cache_dir: /absolute/stt-cache-directory
automatic_download: true
max_audio_bytes: 33554432
max_audio_seconds: 600
queue_capacity: 8
log_level: info

# TTS (all fields optional)
tts:
  provider: tts-rs
  model_id: kokoro-v1.0-int8
  # model_path: /absolute/local/kokoro-directory
  # cache_dir: /absolute/tts-cache-directory
  automatic_download: true
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

Defaults cache STT beneath `$XDG_CACHE_HOME/sophon/models` and TTS beneath `$XDG_CACHE_HOME/sophon/models/tts`. A configured local model path is validated and never replaced by an automatic download. Registry downloads use pinned HTTPS release artifacts, per-file SHA-256 verification, locking, and atomic publication.

The Kokoro int8 model is approximately 88 MiB and its voice archive approximately 27 MiB, for an initial download/cache footprint of roughly 115 MiB, excluding the generated optimized ONNX graph.

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

`SpeakToFile` requires an absolute path that does not exist. Creation is exclusive, concurrent creation returns `OutputExists`, and a failed write removes only the partial file Sophon created.

`SpeakToBuffer` returns a server-created memfd positioned at byte zero. Its complete WAV contents and size are immutable using Linux write, grow, shrink, and further-sealing seals. The transferred client descriptor remains readable after the server drops its reference.

`SpeakAloud` returns only after complete playback drains or fails. Calls are serialized and never overlap.

### TTS options

Options are strict D-Bus variants:

| Key | Type | Meaning |
|---|---|---|
| `voice` | `s` | Named voice advertised by `AvailableVoices` |
| `language` | `s` | Language tag compatible with the selected voice |
| `speed` | `d` | Finite multiplier from `0.5` through `2.0` |
| `clone_audio` | `h` | Transferred canonical reference-WAV descriptor |
| `clone_transcript` | `s` | Optional transcript; requires `clone_audio` |
| `voice_description` | `s` | Provider-specific voice-design intent |

`voice`, `clone_audio`, and `voice_description` are mutually exclusive. Omitted voice and speed use configured defaults. Unknown keys, wrong variant types, unavailable voices, contradictory intents, orphan clone transcripts, invalid language/voice combinations, and invalid speed return `InvalidTtsOptions` before inference is queued.

Kokoro supports default and named voices. It reports cloning and voice design as unsupported; those valid intents return `UnsupportedCapability` without fallback.

### Lifecycle and capability discovery

STT properties are `State`, `ActiveEngine`, `ActiveModel`, `DownloadProgress`, and `LastError`.

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

## Etymology

A Sophon is fictional technology from *Remembrance of Earth's Past* by Cixin Liu, translated by Ken Liu. In *The Three-Body Problem*, Sophons are used to communicate covertly with human scientists and stall scientific progress on Earth.
