## Why

`audio::read_file` currently validates path metadata and then opens the path separately, so a concurrent path replacement can make validation and parsing apply to different filesystem objects. Opening once with nonblocking Unix semantics and validating descriptor metadata closes that check/use gap without allowing a FIFO to stall the D-Bus service.

## What Changes

- Open transcription input paths once with `OpenOptions` and Unix `O_NONBLOCK`, then obtain regular-file and encoded-size metadata from that same `File` before parsing it.
- Continue following symlinks to regular files and preserve all absolute-path, regular-file, byte-limit, duration-limit, WAV, and error contracts.
- Reject FIFOs and other non-regular objects without blocking on their data source.
- Add deterministic tests for descriptor identity and nonblocking special-file rejection.
- Retain the already-present direct `libc` dependency for `O_NONBLOCK`; the separately selected dependency cleanup removes the other six unused entries.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `speech-transcription`: Require file validation and WAV reading to use the same opened filesystem object and reject non-regular objects without waiting for their content.

## Impact

- Implementation: `audio::read_file` and audio ingestion tests in `src/audio.rs`.
- Transport call site: `SophonDbus::transcribe_file` in `src/dbus/mod.rs`; its signature and error mapping remain unchanged.
- Dependency interaction: `Cargo.toml` keeps direct `libc`; `remove-unused-root-dependencies` must remove six, not seven, entries.
- Public compatibility boundary: `read_file(&Path, u64, u64) -> Result<DecodedWav, SophonError>` remains unchanged.
- No D-Bus signature, accepted regular-file/symlink behavior, WAV format, size/duration limit, configuration, persisted-data, or external-state change.
