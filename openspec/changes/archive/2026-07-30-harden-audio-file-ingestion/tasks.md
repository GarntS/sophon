## 1. Open and validate one filesystem object

- [x] 1.1 Add a private `OpenOptions`-based audio opener using Unix `OpenOptionsExt` and `libc::O_NONBLOCK`, while continuing to follow symlinks.
- [x] 1.2 Add a private opened-file helper that uses `File::metadata`, rejects non-regular objects, enforces `max_bytes`, and passes that same owned `File` to `parse_wav`.
- [x] 1.3 Rework public `read_file` to retain its absolute-path check and compose the two helpers without `std::fs::metadata(path)` or a second path open.
- [x] 1.4 Preserve the exact public signature and `InvalidAudio`/`ResourceLimit` classifications used by `SophonDbus::transcribe_file`.

## 2. Cover identity and compatibility cases

- [x] 2.1 Add a deterministic helper-level test that opens valid file A, replaces its pathname with file B, and proves metadata, limits, and parsing still use the open descriptor for A.
- [x] 2.2 Add a Unix FIFO test, creating the fixture with `libc`, that requires `read_file` to return `InvalidAudio` within a bounded timeout without a writer.
- [x] 2.3 Add a symlink-to-regular-WAV test proving accepted symlink behavior is unchanged.
- [x] 2.4 Retain and pass existing relative-path, regular-file, encoded-size, duration, malformed-WAV, and D-Bus transcription tests.

## 3. Coordinate dependency cleanup

- [x] 3.1 Confirm `Cargo.toml` retains direct `libc = "=0.2.178"` and the implementation uses `libc::O_NONBLOCK`.
- [x] 3.2 Apply this change before or together with `remove-unused-root-dependencies`, whose implementation must remove only the other six audited entries.

## 4. Run project validation

- [x] 4.1 Verify by targeted search that `read_file` does not call path metadata or reopen the input path after obtaining its descriptor.
- [x] 4.2 Run `nix develop -c cargo fmt --all -- --check`.
- [x] 4.3 Run `nix develop -c cargo clippy --all-targets -- -D warnings`.
- [x] 4.4 Run `nix develop -c cargo test --workspace`.
- [x] 4.5 Run `nix build .#checks.x86_64-linux.sophon-cpu-qwen-runtime`.
