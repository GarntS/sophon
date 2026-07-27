# Sophon

Sophon is a headless D-Bus speech-to-text and text-to-speech service. Endpoints are provided to transcribe from files on-disk or memfds, to enable cross-process sharing of data without having to write to disk.

For STT, Sophon accepts complete WAV recordings and performs local transcription using the [`transcribe-rs`](https://github.com/cjpais/transcribe-rs.git) Rust crate, the same one used by the [Handy](https://github.com/cjpais/Handy) project. Currently, Sophon supports Parakeet or Canary models for STT, and can run on CPU-accelerated, CUDA-accelerated, or AMD MIGraphX-accelerated ONNX runtimes.

TTS is not-yet-implemented, and will come in the near future

## Install

As this is still a pet project, still in-development, no automated builds are yet provided. During development, this project targets NixOS and provides different Nix flake packages for different ONNX backends:

```sh
nix profile install .#sophon-cpu       # default, CPU only
nix profile install .#sophon-cuda      # CUDA with CPU fallback
nix profile install .#sophon-migraphx  # AMD MIGraphX with CPU fallback
```

## Configuration

Sophon is configured via a YAML-formatted config file. At startup Sophon reads `$XDG_CONFIG_HOME/sophon/config.yaml` to fetch its configuration.

```yaml
engine: parakeet # parakeet | canary
model_id: parakeet-tdt-0.6b-v3-int8
# model_path: /absolute/local/model-directory
quantization: int8 # int8 | fp16 | fp32
accelerator: auto # auto | cpu | cuda | migraphx
language: en
translate: false
# cache_dir: /absolute/cache-directory
automatic_download: true
max_audio_bytes: 33554432
max_audio_seconds: 600
queue_capacity: 8
log_level: info
```

**Sophon attempts to have opinionated, sane defaults:**
- `int8`-quantized Parakeet TDT v3, downloaded automatically
- `auto` hardware acceleration for ONNX
- English language
- Translation disabled
- A 32 MiB audio input queue, a max clip length of 10 minutes, and a queue depth of 8.
- Models are, by default, cached in `$XDG_CACHE_HOME/sophon/models`
- If a user-specified `model_path` is provided, Sophon avoids downloading any models.
- Downloads are HTTPS, verified per file, and atomically published.

## D-Bus API

Name: `com.garntresearch.sophon`  
Path and interface: `/com/garntresearch/sophon`, `com.garntresearch.sophon`

- `TranscribeFile(s path, a{sv} options) -> s`
- `TranscribeMemfd(h fd, a{sv} options) -> s`

Options are strict: `language` is a string and `translate` is a boolean. Omitted values use configuration defaults. Translation is to English and only supported by capable models. Unknown options, wrong types, unsupported languages, and unsupported translation are rejected.

Read-only properties are `State`, `ActiveEngine`, `ActiveModel`, `DownloadProgress`, and `LastError`; standard `PropertiesChanged` signals report lifecycle updates. States progress through `Initializing`, `Downloading`, `Loading`, `Ready`, and `Failed`.

Errors use stable names: `com.garntresearch.sophon.NotReady`, `InvalidOptions`, `InvalidAudio`, `ModelUnavailable`, `ResourceLimit`, and `TranscriptionFailed`. Calls before readiness are retryable; calls after initialization failure return `ModelUnavailable`.

Clients should use a timeout appropriate for local inference; accepted requests are serialized and a caller timing out does not cancel queued inference.

## Audio requirements

Both methods accept only complete RIFF/WAVE data containing mono, 16 kHz, signed 16-bit PCM samples. File paths must be absolute regular files. Transferred descriptors must be readable and seekable; they need not be literal Linux memfds. Inputs exceeding configured encoded-size or decoded-duration limits, malformed WAV data, or unsupported sample properties are rejected before inference.

## Etymology

A Sophon is a piece of fictional technology from the *Rememberance of Earth's Past* book series, written by Cixin Liu, and translated by Ken Liu. In *The 3-Body Problem*, Sophons are used for covertly communicating with human scientists to stall scienfitic progress on Earth.
