"""Per-cell statistics, the stdout report table, and JSONL output.

Measured distributions contain only ``measured`` records: warmup calls and
each cell's flagged first call are reported separately, never folded into
the percentiles. All latencies are labelled end-to-end, and the baseline
cells are identified as the transport floor.
"""

from __future__ import annotations

import json
import math
import statistics
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import TextIO

from .measure import KIND_FIRST, KIND_MEASURED, KIND_WARMUP, CellResult


@dataclass
class CellStats:
    n: int
    minimum: float
    p50: float
    p90: float
    p99: float
    maximum: float
    mean: float
    stddev: float
    rtf_median: float | None


def percentile(sorted_values: list[float], percent: float) -> float:
    """Percentile with linear interpolation between closest ranks."""
    if not sorted_values:
        return float("nan")
    if len(sorted_values) == 1:
        return sorted_values[0]
    rank = (len(sorted_values) - 1) * percent / 100.0
    low = math.floor(rank)
    high = math.ceil(rank)
    if low == high:
        return sorted_values[low]
    fraction = rank - low
    return sorted_values[low] * (1 - fraction) + sorted_values[high] * fraction


def compute_stats(cell: CellResult) -> CellStats | None:
    """Statistics over the cell's measured records only, or None if empty."""
    measured = cell.measured
    if not measured:
        return None
    latencies = sorted(record.latency_s for record in measured)
    rtfs = [record.rtf for record in measured if record.rtf is not None]
    return CellStats(
        n=len(latencies),
        minimum=latencies[0],
        p50=percentile(latencies, 50),
        p90=percentile(latencies, 90),
        p99=percentile(latencies, 99),
        maximum=latencies[-1],
        mean=statistics.fmean(latencies),
        stddev=statistics.stdev(latencies) if len(latencies) > 1 else 0.0,
        rtf_median=statistics.median(rtfs) if rtfs else None,
    )


def _fmt_seconds(value: float) -> str:
    if value != value:  # NaN
        return "-"
    if value < 1.0:
        return f"{value * 1000:.1f}ms"
    if value < 100.0:
        return f"{value:.2f}s"
    return f"{value:.0f}s"


def _fmt_rtf(value: float | None) -> str:
    if value is None or value != value:
        return "-"
    return f"{value:.2f}"


def print_report(
    cells: list[CellResult],
    partial: bool,
    baseline_valid: bool,
    out: TextIO = sys.stdout,
) -> None:
    """Print the per-cell percentile table to stdout."""
    print(file=out)
    if partial:
        print("*** PARTIAL RESULTS (run interrupted) ***", file=out)
    print(
        "All latencies are end-to-end (client-measured: bus round trip, "
        "queueing, inference, and output).",
        file=out,
    )
    print(
        "baseline/* rows time pre-queue rejections: the IPC + validation "
        "transport floor, with no inference.",
        file=out,
    )
    if not baseline_valid:
        print(
            "WARNING: at least one baseline call did not return the expected "
            "pre-queue rejection; those measurements were excluded and the "
            "baseline is invalid.",
            file=out,
        )

    header = (
        f"{'cell':<28} {'n':>4} {'min':>9} {'p50':>9} {'p90':>9} "
        f"{'p99':>9} {'max':>9} {'mean':>9} {'sd':>9} {'rtf~p50':>8}"
    )
    print(file=out)
    print(header, file=out)
    print("-" * len(header), file=out)
    for cell in cells:
        stats = compute_stats(cell)
        if stats is None:
            failures = len(cell.failures)
            detail = f" ({failures} failed)" if failures else ""
            print(f"{cell.name:<28} {'0':>4}   no measured data{detail}", file=out)
            continue
        print(
            f"{cell.name:<28} {stats.n:>4} "
            f"{_fmt_seconds(stats.minimum):>9} {_fmt_seconds(stats.p50):>9} "
            f"{_fmt_seconds(stats.p90):>9} {_fmt_seconds(stats.p99):>9} "
            f"{_fmt_seconds(stats.maximum):>9} {_fmt_seconds(stats.mean):>9} "
            f"{_fmt_seconds(stats.stddev):>9} {_fmt_rtf(stats.rtf_median):>8}",
            file=out,
        )
        if cell.failures:
            print(
                f"  ! {len(cell.failures)} call(s) failed or invalid; "
                "excluded from the distribution",
                file=out,
            )

    separated = [
        (cell, record)
        for cell in cells
        for record in cell.records
        if record.kind in {KIND_FIRST, KIND_WARMUP}
    ]
    if separated:
        print(file=out)
        print(
            "Warmup and first-call data (excluded from the distributions above):",
            file=out,
        )
        current = None
        for cell, record in separated:
            if cell.name != current:
                if current is not None:
                    print(file=out)
                print(f"  {cell.name}:", end="", file=out)
                current = cell.name
            tag = record.kind
            value = "ERROR" if record.error else _fmt_seconds(record.latency_s)
            print(f" [{tag} {value}]", end="", file=out)
        print(file=out)


class JsonlWriter:
    """Streaming JSON-lines writer: manifest first, then measured calls.

    Records are appended and flushed as calls complete, so the file keeps
    partial results when a run is interrupted.
    """

    def __init__(self, path: Path, manifest: dict):
        self._handle = path.open("w", encoding="utf-8")
        self._write(manifest)

    def _write(self, record: dict) -> None:
        self._handle.write(json.dumps(record) + "\n")
        self._handle.flush()

    def write_call(self, record) -> None:
        if record.kind == KIND_MEASURED:
            self._write(record.to_json())

    def close(self) -> None:
        self._handle.close()
