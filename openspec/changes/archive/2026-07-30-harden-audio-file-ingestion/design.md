## Context

`read_file` currently performs `std::fs::metadata(path)`, checks regular-file status and encoded size, then calls `File::open(path)` and parses that second resolution. D-Bus callers control the absolute input path, so concurrent rename or symlink changes can make the checked object differ from the parsed object.

Simply reversing the two calls is unsafe because a read-only open of a FIFO can block before metadata rejects it. The project targets Unix/NixOS and already declares `libc`; the user chose to retain that direct dependency so the open can use `O_NONBLOCK` without introducing another crate.

## Goals / Non-Goals

**Goals:**

- Validate regular-file type and encoded size on the exact descriptor passed to WAV parsing.
- Ensure opening a FIFO does not wait for a writer.
- Preserve accepted absolute regular files and symlinks to regular files.
- Preserve the public function signature, error variants, limits, and downstream D-Bus behavior.

**Non-Goals:**

- No sandboxing or new path allowlist.
- No change to descriptor-based `read_unix_fd`, clone audio, WAV decoding, downmixing, or resampling.
- No change to file permissions, symlink-following policy, byte/duration limits, or error text except where a platform open error already supplies its diagnostic.
- No new dependency; direct `libc` remains and is removed from the separate cleanup target list.

## Decisions

1. **Open once with nonblocking Unix flags.** Add a private opener using `std::fs::OpenOptions`, `std::os::unix::fs::OpenOptionsExt`, `.read(true)`, and `.custom_flags(libc::O_NONBLOCK)`. Rust retains its normal close-on-exec handling. `O_NONBLOCK` has no effect on regular-file reads but prevents a FIFO open from waiting for a peer.

2. **Inspect descriptor metadata.** After the absolute-path check, `read_file` opens exactly once, calls `File::metadata()` on that descriptor, rejects non-regular files, applies `max_bytes`, and passes the same owned `File` to `parse_wav`. It SHALL not call `std::fs::metadata(path)` or reopen the path.

   Before:

   ```text
   path -> metadata(path) -> File::open(path) -> parse
   ```

   After:

   ```text
   path -> OpenOptions(O_NONBLOCK) -> file.metadata() -> parse(same file)
   ```

3. **Use small private helpers to test identity deterministically.** Separate `open_audio_file(path)` from `read_open_audio_file(file, limits)`. Production `read_file` composes them directly. A unit test can open file A, replace the pathname with file B, and pass the already-open descriptor to the second helper, proving metadata and parsing remain attached to A without adding production hooks or public API.

4. **Retain symlink behavior.** Do not set `O_NOFOLLOW`; opening an absolute symlink to a regular file continues to follow its target, and descriptor metadata verifies that target. Broken/inaccessible symlinks continue returning `InvalidAudio` through the open error.

5. **Retain `libc` as an earned direct dependency.** `harden-audio-file-ingestion` should be applied before or together with `remove-unused-root-dependencies`; that change removes six entries and explicitly keeps `libc` for `O_NONBLOCK`.

## Risks / Trade-offs

- **[Risk] A special file's open has device-specific side effects despite `O_NONBLOCK`.** → Immediately reject descriptor metadata unless it is a regular file; this change improves the current race but is not a filesystem sandbox.
- **[Risk] Platform-specific code breaks non-Unix builds.** → The crate's service and tests already target Unix/Linux D-Bus, memfd, and PipeWire behavior; keep the import localized and validate the supported targets.
- **[Risk] Symlink compatibility changes accidentally.** → Add a symlink-to-regular-file acceptance test and avoid `O_NOFOLLOW`.
- **[Risk] FIFO regression hangs the test suite.** → Run the FIFO call in a helper thread and require an `InvalidAudio` result within a short timeout.
- **[Trade-off] `libc` is no longer removable.** → It is already pinned and transitive in the graph, and using its platform constant is lighter than adding a replacement crate or accepting blocking behavior.

## Migration Plan

1. Add the private nonblocking opener and opened-file validator/parser in `src/audio.rs`.
2. Rework `read_file` to compose those helpers with one path open.
3. Extend audio tests for same-descriptor identity, FIFO nonblocking rejection, symlink compatibility, size limits, and existing regular-file behavior.
4. Run Rust, D-Bus integration, and CPU Nix validation.
5. Apply `remove-unused-root-dependencies` with `libc` excluded from removal.

Rollback restores path metadata followed by a second open. There is no data, configuration, or deployment migration.
