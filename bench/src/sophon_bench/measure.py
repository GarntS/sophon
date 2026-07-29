"""Shared measurement loop and per-cell records.

Every measured cell (baseline rejection, STT transcription, TTS synthesis)
runs through the same loop: an optional separately classified first call,
``warmup`` unmeasured calls, then ``reps`` measured calls. Calls execute strictly
sequentially and are timed with ``time.monotonic_ns`` around the awaited D-Bus
call. Records stream to a callback as they complete so partial results survive
an interrupt.
"""

from __future__ import annotations

import sys
import time
from dataclasses import dataclass, field
from typing import Any, Awaitable, Callable

from .client import DaemonError

KIND_FIRST = "first"
KIND_WARMUP = "warmup"
KIND_MEASURED = "measured"


class UnexpectedSuccess(Exception):
    """A call expected to be rejected by the daemon succeeded instead."""


@dataclass
class CallRecord:
    cell: str
    kind: str
    first: bool
    latency_s: float
    rtf: float | None
    axes: dict[str, Any]
    error: str | None = None
    invalid: bool = False

    def to_json(self) -> dict:
        record = {
            "type": "call",
            "cell": self.cell,
            "kind": self.kind,
            "first": self.first,
            "axes": self.axes,
            "latency_s": self.latency_s,
            "rtf": self.rtf,
        }
        if self.error is not None:
            record["error"] = self.error
        if self.invalid:
            record["invalid"] = True
        return record


@dataclass
class CellResult:
    name: str
    axes: dict[str, Any]
    records: list[CallRecord] = field(default_factory=list)

    @property
    def measured(self) -> list[CallRecord]:
        return [
            r
            for r in self.records
            if r.kind == KIND_MEASURED and r.error is None and not r.invalid
        ]

    @property
    def failures(self) -> list[CallRecord]:
        return [r for r in self.records if r.error is not None or r.invalid]


# An async callable performing one daemon call and returning an opaque result.
CallFn = Callable[[], Awaitable[Any]]
# Maps (latency, call result) to a per-call real-time factor, or None.
RtfFn = Callable[[float, Any], "float | None"]
RecordSink = Callable[[CallRecord], None]


def _print_progress(record: CallRecord, index: int, total: int) -> None:
    tag = "first" if record.first else record.kind
    status = ""
    if record.error is not None:
        status = f" ERROR ({record.error})"
    elif record.invalid:
        status = " INVALID (unexpected success)"
    rtf = f" rtf={record.rtf:.2f}" if record.rtf is not None else ""
    print(
        f"  [{record.cell}] {tag} {index}/{total}: "
        f"{record.latency_s:.3f}s{rtf}{status}",
        file=sys.stderr,
        flush=True,
    )


async def run_cell(
    name: str,
    axes: dict[str, Any],
    warmup: int,
    reps: int,
    call_fn: CallFn,
    rtf_fn: RtfFn,
    on_record: RecordSink | None = None,
    collector: list[CellResult] | None = None,
    record_first: bool = False,
    cell: CellResult | None = None,
) -> CellResult:
    """Run an optional first call, warmups, then measured repetitions.

    Sequential by construction: each call is awaited before the next starts.
    Per-call failures (unexpected daemon errors, invalid baseline successes)
    are recorded and excluded from statistics without aborting the cell.
    When a collector list is given, the cell is registered before any call
    runs so partial results survive an interrupt.
    """
    if cell is None:
        cell = CellResult(name=name, axes=axes)
        if collector is not None:
            collector.append(cell)
    total = int(record_first) + warmup + reps
    for index in range(total):
        if record_first and index == 0:
            kind = KIND_FIRST
        elif index < int(record_first) + warmup:
            kind = KIND_WARMUP
        else:
            kind = KIND_MEASURED
        first = kind == KIND_FIRST
        start_ns = time.monotonic_ns()
        result: Any = None
        error: str | None = None
        invalid = False
        try:
            result = await call_fn()
        except UnexpectedSuccess as problem:
            invalid = True
            error = str(problem)
        except DaemonError as problem:
            error = f"{problem.name}: {problem.message}"
        latency_s = (time.monotonic_ns() - start_ns) / 1e9
        rtf = None if error is not None else rtf_fn(latency_s, result)
        record = CallRecord(
            cell=name,
            kind=kind,
            first=first,
            latency_s=latency_s,
            rtf=rtf,
            axes=axes,
            error=error,
            invalid=invalid,
        )
        cell.records.append(record)
        _print_progress(record, index + 1, total)
        if on_record is not None:
            on_record(record)
    return cell
