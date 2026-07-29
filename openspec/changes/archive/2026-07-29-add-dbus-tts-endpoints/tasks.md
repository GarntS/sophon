## 1. Dependencies and Packaging

- [x] 1.1 Add `tts-rs` with Kokoro support and align its `ort` resolution with Sophon's pinned dynamically supplied ONNX Runtime; verify a minimal Kokoro build without runtime downloads
- [x] 1.2 Add direct PipeWire and Linux memfd dependencies with the required Tokio/zbus feature wiring
- [x] 1.3 Extend Nix build inputs and runtime closures with PipeWire and `espeak-ng`, preserving the accelerator-specific ONNX Runtime selection
- [x] 1.4 Update Nix closure and build checks to assert the intended TTS, PipeWire, espeak, and ONNX Runtime dependencies

## 2. TTS Domain and Configuration

- [x] 2.1 Add provider-neutral TTS request, voice-intent, capability, owned-audio, lifecycle, and public error domain types
- [x] 2.2 Add the strict nested TTS configuration structure and documented Kokoro, voice, speed, playback, cache, queue, and resource-limit defaults
- [x] 2.3 Validate provider/model combinations, paths, voice and node strings, finite speed and volume ranges, and bounded nonzero TTS limits while keeping STT configuration independently usable
- [x] 2.4 Add configuration tests covering omitted, partial, complete, unknown, malformed, out-of-range, and STT-independent TTS settings

## 3. Model Acquisition and Provider Layer

- [x] 3.1 Generalize or extend verified acquisition so an independent TTS registry can reuse locked download, SHA-256 validation, complete-layout validation, and atomic cache publication
- [x] 3.2 Pin the tested Kokoro int8 ONNX and voice artifacts to immutable upstream locations with verified SHA-256 digests and exact expected relative paths
- [x] 3.3 Add tests for valid TTS cache reuse, complete automatic download, checksum/interruption failure, and terminal invalid local overrides
- [x] 3.4 Define the mutable Sophon TTS provider interface and capability validation for default, named, clone, and voice-design intents
- [x] 3.5 Implement the `tts-rs` Kokoro adapter with model loading, owned voice enumeration, named-voice/language/speed validation, owned 24 kHz float PCM results, and explicit unsupported clone/design capabilities
- [x] 3.6 Implement a bounded FIFO TTS worker that serializes provider inference, rejects a full queue, survives per-request failures, and validates generated output duration
- [x] 3.7 Add provider and worker fixture tests for option mapping, capability rejection, FIFO behavior, queue exhaustion, output limits, and continued operation after inference failure

## 4. Reference Input and WAV Outputs

- [x] 4.1 Add strict decoding for readable seekable mono 24 kHz 32-bit IEEE-float clone WAV descriptors from offset zero with encoded-byte and decoded-duration limits
- [x] 4.2 Add provider-neutral mono float WAV encoding that validates sample rate, finite samples, frame counts, and generated-output limits
- [x] 4.3 Implement exclusive absolute-path publication that never replaces an existing object, reports concurrent creation, and removes files partially created by failed writes
- [x] 4.4 Implement server-created sealing-enabled memfd output that finalizes the WAV, reports encoded length, rewinds to zero, and applies write/grow/shrink/seal seals
- [x] 4.5 Add unit tests for canonical and malformed clone WAVs, file exclusivity and cleanup, complete float WAV metadata, memfd offset and length, enforced seals, and descriptor lifetime after server ownership is dropped

## 5. PipeWire Playback

- [x] 5.1 Introduce a testable playback interface accepting owned mono float PCM, sample rate, optional stable PipeWire node name, and linear volume
- [x] 5.2 Implement direct PipeWire output that uses the default sink only when no node is configured, targets an exact configured `node.name`, and waits for stream drain or failure
- [x] 5.3 Add a bounded serialized FIFO playback stage so accepted `SpeakAloud` calls never overlap and playback failure does not alter model readiness
- [x] 5.4 Add fixture tests for volume scaling, default and explicit device selection, missing explicit devices, synchronous completion, FIFO serialization, and recovery after playback failure
- [x] 5.5 Add a Nix-environment PipeWire smoke check or documented test harness that verifies stream negotiation and complete drain against a controlled PipeWire instance

## 6. Service, D-Bus, and Daemon Composition

- [x] 6.1 Add strict D-Bus TTS option decoding for string, double, and transferred-FD variants, including mutual-exclusion, clone-transcript, text-size, voice, language, and speed validation
- [x] 6.2 Add a transport-independent TTS service that checks independent readiness, submits bounded synthesis, and dispatches exclusive file, sealed memfd, or serialized playback output
- [x] 6.3 Extend the D-Bus error enum and domain mapping with all stable TTS-specific names while retaining shared readiness and resource errors
- [x] 6.4 Export `SpeakToFile`, `SpeakToBuffer`, and `SpeakAloud` with the specified typed signatures and return values
- [x] 6.5 Add independent `TtsState`, `ActiveTtsProvider`, `ActiveTtsModel`, `TtsDownloadProgress`, `TtsLastError`, `AvailableVoices`, and `TtsCapabilities` properties and `PropertiesChanged` emission
- [x] 6.6 Start STT and TTS configuration, acquisition, loading, and lifecycle observation independently after claiming the D-Bus name; install each service without coupling the other's failure state

## 7. Contract Tests and Documentation

- [x] 7.1 Expand isolated D-Bus integration tests to verify introspection signatures, independent lifecycle properties, strict option types, readiness, named voices, unsupported cloning, queue limits, and stable error names
- [x] 7.2 Add D-Bus integration coverage for exclusive `SpeakToFile`, sealed transferred `SpeakToBuffer` descriptor contents/lifetime, and synchronous serialized `SpeakAloud` through a playback fixture
- [x] 7.3 Add daemon initialization tests proving TTS failure leaves ready STT transcription usable and STT failure does not overwrite TTS lifecycle state
- [x] 7.4 Update README configuration, D-Bus API, option matrix, WAV/reference formats, model download footprint, PipeWire device/volume behavior, capability discovery, timeout, and error documentation
- [x] 7.5 Run formatting, clippy with warnings denied, unit and D-Bus integration tests, Nix package builds, accelerator evaluation checks, and closure-policy checks
