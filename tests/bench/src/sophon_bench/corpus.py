"""STT audio corpus acquisition.

The corpus is a set of 16 kHz mono WAV files bucketed by duration around
~1s/3s/10s/30s targets. It comes from a user-supplied ``--wav-dir`` when
given; otherwise it is synthesized through the daemon's own TTS, resampled to
16 kHz mono with an externally detected resampler (ffmpeg or sox, never a
hard dependency), and cached under the user's XDG cache directory for reuse.
"""

from __future__ import annotations

import math
import os
import shutil
import subprocess
import sys
import wave
from dataclasses import dataclass
from pathlib import Path

from .client import SophonClient

TARGET_DURATIONS_S = (1.0, 3.0, 10.0, 30.0)

# Fixed prompt sentences for fallback synthesis, roughly paced to hit the
# duration targets. Exact wording is not load-bearing: cells are bucketed and
# labelled by the *measured* duration of the resulting audio.
PROMPTS = {
    1.0: "Hello, world.",
    3.0: "The quick brown fox jumps over the lazy dog near the riverbank.",
    10.0: (
        "Benchmarks are most useful when they reflect real workloads, so this "
        "sentence is meant to sound like natural spoken language rather than "
        "a synthetic tongue twister read at an unnatural pace."
    ),
    30.0: (
        "Performance measurement is easy to do badly and surprisingly hard to "
        "do well. Every number in a report like this one is the sum of many "
        "small costs: encoding the request, crossing the process boundary, "
        "waiting for a worker, running the model, and packaging the result. "
        "None of those steps is especially interesting on its own, but "
        "together they decide whether a system feels instant or sluggish. "
        "Reading a paragraph this long out loud takes about half a minute, "
        "which makes it a useful stand-in for dictating a short note or a "
        "couple of chat messages in one go."
    ),
}

# Daemon input constraint: mono 16 kHz signed 16-bit PCM WAV.
_REQUIRED_CHANNELS = 1
_REQUIRED_RATE = 16_000
_REQUIRED_WIDTH = 2


class CorpusError(Exception):
    """A corpus file violates the daemon's input constraints."""


@dataclass
class CorpusEntry:
    target_s: float
    duration_s: float
    path: Path


def _note(message: str) -> None:
    print(f"sophon-bench: {message}", file=sys.stderr)


def validate_wav(path: Path) -> float:
    """Return the duration in seconds of a valid corpus file.

    Raises CorpusError naming the violated constraint when the file is not a
    16 kHz mono 16-bit PCM WAV.
    """
    try:
        with wave.open(str(path), "rb") as wav:
            channels = wav.getnchannels()
            rate = wav.getframerate()
            width = wav.getsampwidth()
            frames = wav.getnframes()
    except (wave.Error, EOFError, OSError) as error:
        raise CorpusError(f"{path}: not a readable RIFF/WAVE file ({error})")
    if channels != _REQUIRED_CHANNELS:
        raise CorpusError(
            f"{path}: expected mono ({_REQUIRED_CHANNELS} channel), got {channels} channels"
        )
    if rate != _REQUIRED_RATE:
        raise CorpusError(
            f"{path}: expected {_REQUIRED_RATE} Hz sample rate, got {rate} Hz"
        )
    if width != _REQUIRED_WIDTH:
        raise CorpusError(
            f"{path}: expected {_REQUIRED_WIDTH * 8}-bit PCM samples, got {width * 8}-bit"
        )
    return frames / rate


def _bucket_target(duration_s: float) -> float:
    """Nearest duration target on a log scale."""
    return min(
        TARGET_DURATIONS_S,
        key=lambda target: abs(math.log(max(duration_s, 1e-9) / target)),
    )


def load_wav_dir(wav_dir: Path) -> list[CorpusEntry]:
    """Load and validate a user-supplied corpus directory.

    Files violating the 16 kHz mono WAV constraint are rejected with a
    message naming the constraint, before any benchmark call can use them.
    Accepted files are bucketed by duration; per bucket the file closest to
    the bucket target represents the cell.
    """
    candidates = sorted(
        path for path in wav_dir.iterdir() if path.suffix.lower() == ".wav"
    )
    if not candidates:
        raise CorpusError(f"{wav_dir}: no .wav files found")
    buckets: dict[float, CorpusEntry] = {}
    for path in candidates:
        try:
            duration_s = validate_wav(path)
        except CorpusError as error:
            _note(f"rejecting corpus file: {error}")
            continue
        target = _bucket_target(duration_s)
        current = buckets.get(target)
        if current is None or abs(duration_s - target) < abs(
            current.duration_s - target
        ):
            buckets[target] = CorpusEntry(target, duration_s, path)
    entries = [buckets[target] for target in TARGET_DURATIONS_S if target in buckets]
    if len(entries) < 3:
        raise CorpusError(
            f"{wav_dir}: corpus spans only {len(entries)} duration bucket(s); "
            "at least 3 distinct buckets near 1s/3s/10s/30s are required"
        )
    return entries


def xdg_cache_dir() -> Path:
    """The XDG cache directory used for the generated corpus."""
    base = os.environ.get("XDG_CACHE_HOME") or str(Path.home() / ".cache")
    return Path(base) / "sophon" / "bench-corpus"


def _cache_file(cache_dir: Path, target: float) -> Path:
    return cache_dir / f"{target:g}s.wav"


def load_cached_corpus(cache_dir: Path) -> dict[float, CorpusEntry]:
    """Return valid cached corpus entries, keyed by duration target."""
    entries: dict[float, CorpusEntry] = {}
    for target in TARGET_DURATIONS_S:
        path = _cache_file(cache_dir, target)
        if not path.is_file():
            continue
        try:
            duration_s = validate_wav(path)
        except CorpusError:
            _note(f"discarding invalid cached corpus file: {path}")
            continue
        entries[target] = CorpusEntry(target, duration_s, path)
    return entries


def find_resampler() -> str | None:
    """Detect a supported external resampler on PATH, if any."""
    for tool in ("ffmpeg", "sox"):
        if shutil.which(tool):
            return tool
    return None


def _resample(tool: str, source: Path, dest: Path) -> None:
    """Convert a 24 kHz float WAV to 16 kHz mono 16-bit PCM WAV."""
    if tool == "ffmpeg":
        command = [
            "ffmpeg",
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-i",
            str(source),
            "-ar",
            "16000",
            "-ac",
            "1",
            "-c:a",
            "pcm_s16le",
            str(dest),
        ]
    else:
        command = [
            "sox",
            str(source),
            "-r",
            "16000",
            "-c",
            "1",
            "-b",
            "16",
            "-e",
            "signed-integer",
            str(dest),
        ]
    result = subprocess.run(command, capture_output=True, text=True)
    if result.returncode != 0:
        raise CorpusError(f"{tool} resample failed: {result.stderr.strip()}")


async def synthesize_corpus(
    client: SophonClient,
    missing: list[float],
    cache_dir: Path,
    work_dir: Path,
) -> dict[float, CorpusEntry]:
    """Synthesize and cache corpus entries for the given duration targets."""
    resampler = find_resampler()
    if resampler is None:
        raise CorpusError("no resampler available")
    cache_dir.mkdir(parents=True, exist_ok=True)
    entries: dict[float, CorpusEntry] = {}
    for target in missing:
        raw_path = work_dir / f"corpus-{target:g}s-24k.wav"
        _note(f"synthesizing corpus prompt (~{target:g}s) via daemon TTS")
        await client.speak_to_file(PROMPTS[target], str(raw_path))
        cache_path = _cache_file(cache_dir, target)
        _resample(resampler, raw_path, cache_path)
        duration_s = validate_wav(cache_path)
        entries[target] = CorpusEntry(target, duration_s, cache_path)
    return entries


async def acquire_corpus(
    args,
    client: SophonClient,
    tts_ready: bool,
    work_dir: Path,
) -> list[CorpusEntry] | None:
    """Acquire the STT corpus, or None when the STT sweep must be skipped.

    Priority: ``--wav-dir``; then the XDG cache; then synthesis through the
    daemon's own TTS plus a detected resampler. When none of those can
    produce a corpus, the STT sweep is skipped with an actionable message.
    """
    if args.wav_dir is not None:
        if not args.wav_dir.is_dir():
            raise CorpusError(f"--wav-dir {args.wav_dir} is not a directory")
        entries = load_wav_dir(args.wav_dir)
        if not entries:
            _note(
                f"STT sweep skipped: no usable 16 kHz mono WAV files in {args.wav_dir}"
            )
            return None
        return entries

    cache_dir = xdg_cache_dir()
    entries = load_cached_corpus(cache_dir)
    missing = [target for target in TARGET_DURATIONS_S if target not in entries]
    if not missing:
        _note(f"reusing cached STT corpus from {cache_dir}")
        return [entries[target] for target in TARGET_DURATIONS_S]

    resampler = find_resampler()
    if not tts_ready or resampler is None:
        reasons = []
        if not tts_ready:
            reasons.append("daemon TTS is not ready")
        if resampler is None:
            reasons.append("no resampler (ffmpeg or sox) found on PATH")
        _note(
            "STT sweep skipped: no --wav-dir supplied, no complete cached "
            f"corpus, and {' and '.join(reasons)}. Provide --wav-dir with "
            "16 kHz mono WAV files, or install ffmpeg/sox and rerun against a "
            "TTS-ready daemon to generate and cache a corpus."
        )
        return None

    generated = await synthesize_corpus(client, missing, cache_dir, work_dir)
    entries.update(generated)
    return [entries[target] for target in TARGET_DURATIONS_S if target in entries]
