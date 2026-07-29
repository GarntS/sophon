"""STT and TTS measurement sweeps built on the shared cell loop.

STT cells run ``TranscribeFile`` over the corpus duration buckets with
per-call RTF = audio duration / call latency. TTS cells run the three
embedded text lengths through both ``SpeakToFile`` and ``SpeakToBuffer``
with per-call RTF = call latency / generated audio duration, where the
duration derives from each call's own returned byte size.
"""

from __future__ import annotations

import os
import sys
from pathlib import Path

from .client import SophonClient
from .corpus import CorpusEntry
from .measure import CellResult, RecordSink, run_cell

# TTS output is mono 24 kHz 32-bit IEEE-float WAV behind a standard header.
TTS_BYTES_PER_SECOND = 96_000
WAV_HEADER_BYTES = 44

# Embedded benchmark texts at three lengths. The page text stays near the
# documented ~1500-character cap so CPU inference times remain tolerable.
TTS_TEXTS = {
    "sentence": "The quick brown fox jumps over the lazy dog.",
    "paragraph": (
        "Latency numbers only mean something when you know how they were "
        "measured. Every call in this report is timed from the client side, "
        "so it includes request encoding, the bus round trip, queueing, "
        "inference, and output encoding. That is exactly what a real caller "
        "experiences, which is the point of the exercise."
    ),
    "page": (
        "A good benchmark is a letter from one machine to its future self. "
        "It says: on this day, with this model, this backend, and this much "
        "patience, a spoken sentence took this long to arrive. The details "
        "matter because speech systems are chains of small, boring steps, "
        "and the chain is only as fast as its slowest link. First the text "
        "is checked against the rules of the interface. Then it waits its "
        "turn, because the worker handles one request at a time in the "
        "order they arrive. Then the model does its work, turning words "
        "into a stream of numbers, and numbers into sound. Finally the "
        "sound is wrapped in a file and handed back across the bus. None of "
        "these steps is glamorous, and none of them can be skipped. When "
        "the numbers look strange, the cause is usually mundane: a busy "
        "machine, a cold cache, a governor that parked the clock speed too "
        "low, or another process holding the accelerator. So the sensible "
        "way to read a table of latencies is not as a verdict but as a "
        "snapshot. Run it again when the machine is idle. Run it after a "
        "model change. Compare medians before you compare tails, and treat "
        "the ninetieth percentile as a promise about bad days rather than "
        "typical ones. A sentence of speech is short, a paragraph is a "
        "breath, and a page like this one is a small speech. If the system "
        "can produce this page in less time than it takes to read aloud, it "
        "is faster than real time, and that is the property every speech "
        "tool ultimately has to earn."
    ),
}


async def run_stt_sweep(
    client: SophonClient,
    corpus: list[CorpusEntry],
    warmup: int,
    reps: int,
    cells: list[CellResult],
    on_record: RecordSink | None = None,
) -> None:
    """Measure TranscribeFile latency across the corpus duration buckets."""
    for index, entry in enumerate(corpus):
        name = f"stt/{entry.target_s:g}s"
        axes = {
            "target_s": entry.target_s,
            "duration_s": entry.duration_s,
            "file": entry.path.name,
        }

        async def call(path=entry.path) -> None:
            await client.transcribe_file(str(path))

        def rtf(latency_s: float, _result: None, duration_s=entry.duration_s) -> float:
            return duration_s / latency_s

        await run_cell(
            name,
            axes,
            warmup,
            reps,
            call,
            rtf,
            on_record,
            collector=cells,
            record_first=index == 0,
        )


def _tts_rtf(latency_s: float, size_bytes: int) -> float:
    audio_seconds = max(size_bytes - WAV_HEADER_BYTES, 0) / TTS_BYTES_PER_SECOND
    return latency_s / audio_seconds if audio_seconds > 0 else float("nan")


async def run_tts_first_call(
    client: SophonClient,
    work_dir: Path,
    cells: list[CellResult],
    on_record: RecordSink | None = None,
) -> CellResult:
    """Record the first post-ready TTS inference before corpus synthesis."""
    text = TTS_TEXTS["sentence"]
    axes = {"length": "sentence", "chars": len(text), "mode": "file"}

    async def call_file() -> int:
        path = work_dir / "tts-sentence-first.wav"
        return await client.speak_to_file(text, str(path))

    return await run_cell(
        "tts/sentence/file",
        axes,
        warmup=0,
        reps=0,
        call_fn=call_file,
        rtf_fn=_tts_rtf,
        on_record=on_record,
        collector=cells,
        record_first=True,
    )


async def run_tts_sweep(
    client: SophonClient,
    fd_passing: bool,
    warmup: int,
    reps: int,
    work_dir: Path,
    cells: list[CellResult],
    first_file_cell: CellResult,
    on_record: RecordSink | None = None,
) -> None:
    """Measure TTS latency over text lengths x {file, buffer} output modes."""
    counter = 0
    for label, text in TTS_TEXTS.items():
        axes = {"length": label, "chars": len(text)}

        async def call_file(text=text, label=label) -> int:
            nonlocal counter
            counter += 1
            # SpeakToFile requires an absolute path that does not yet exist.
            path = work_dir / f"tts-{label}-{counter:04d}.wav"
            return await client.speak_to_file(text, str(path))

        existing_cell = first_file_cell if label == "sentence" else None
        await run_cell(
            f"tts/{label}/file",
            {**axes, "mode": "file"},
            warmup,
            reps,
            call_file,
            _tts_rtf,
            on_record,
            collector=cells,
            cell=existing_cell,
        )

        if not fd_passing:
            continue

        async def call_buffer(text=text) -> int:
            fd, size = await client.speak_to_buffer(text)
            os.close(fd)
            return size

        await run_cell(
            f"tts/{label}/buffer",
            {**axes, "mode": "buffer"},
            warmup,
            reps,
            call_buffer,
            _tts_rtf,
            on_record,
            collector=cells,
        )

    if not fd_passing:
        print(
            "sophon-bench: Unix FD passing unavailable on this bus connection; "
            "SpeakToBuffer cells skipped",
            file=sys.stderr,
        )
    return cells
