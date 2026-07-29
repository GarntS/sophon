"""IPC/validation baseline: timed pre-queue rejection calls.

These calls are rejected by the daemon before inference is queued, so their
latency is the transport floor every other measurement sits on top of. Each
timed call is verified to fail with the expected error type; a call that
unexpectedly succeeds is flagged and excluded from the baseline statistics.
"""

from __future__ import annotations

from pathlib import Path

from dbus_next.signature import Variant

from .client import DaemonError, InvalidAudioError, InvalidTtsOptionsError, SophonClient
from .measure import CallFn, CellResult, RecordSink, UnexpectedSuccess, run_cell

_NO_RTF = lambda latency_s, result: None  # noqa: E731


def _rejection_call(
    method: CallFn,
    expected: type[DaemonError],
) -> CallFn:
    """Wrap a daemon call expected to fail with a specific error type."""

    async def call() -> None:
        try:
            await method()
        except expected:
            return None
        raise UnexpectedSuccess(
            f"expected {expected.__name__} rejection but the call succeeded"
        )

    return call


async def run_baseline(
    client: SophonClient,
    tts_ready: bool,
    warmup: int,
    reps: int,
    work_dir: Path,
    cells: list[CellResult],
    on_record: RecordSink | None = None,
) -> None:
    """Time the pre-queue rejection calls that make up the transport floor."""
    # Rejected as invalid audio before inference is queued: the path does not
    # exist, so validation fails in the pre-queue input checks.
    missing_audio = work_dir / "nonexistent-input.wav"
    await run_cell(
        name="baseline/invalid-audio",
        axes={"rejection": "invalid-audio"},
        warmup=warmup,
        reps=reps,
        call_fn=_rejection_call(
            lambda: client.transcribe_file(str(missing_audio)),
            InvalidAudioError,
        ),
        rtf_fn=_NO_RTF,
        on_record=on_record,
        collector=cells,
    )

    if tts_ready:
        # Rejected as invalid options before inference is queued: the option
        # key is unknown, so TTS option decode fails before any output work.
        unknown_options = {"sophon_bench_unknown_option": Variant("s", "x")}
        await run_cell(
            name="baseline/invalid-tts-options",
            axes={"rejection": "invalid-tts-options"},
            warmup=warmup,
            reps=reps,
            call_fn=_rejection_call(
                lambda: client.speak_to_file(
                    "Baseline rejection probe.",
                    str(work_dir / "never-created-output.wav"),
                    unknown_options,
                ),
                InvalidTtsOptionsError,
            ),
            rtf_fn=_NO_RTF,
            on_record=on_record,
            collector=cells,
        )


def baseline_is_valid(cells: list[CellResult]) -> bool:
    """False when any baseline call failed to reject with the expected type."""
    return not any(cell.failures for cell in cells)
