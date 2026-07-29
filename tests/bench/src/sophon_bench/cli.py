"""Command-line entry point for the sophon benchmark harness."""

from __future__ import annotations

import argparse
import asyncio
import sys
from pathlib import Path

from . import __version__


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="sophon-bench",
        description=(
            "Black-box latency benchmarks for a running sophon daemon. "
            "Discovers the live daemon configuration over the session bus, "
            "waits for readiness, and measures an IPC baseline plus STT and "
            "TTS end-to-end latency. The daemon is never started, stopped, "
            "or reconfigured."
        ),
    )
    parser.add_argument(
        "--wav-dir",
        type=Path,
        default=None,
        metavar="DIR",
        help=(
            "directory of 16 kHz mono WAV files to use as the STT corpus; "
            "files are bucketed by duration (~1s/3s/10s/30s). When omitted, "
            "a corpus is synthesized through the daemon's own TTS and cached."
        ),
    )
    parser.add_argument(
        "--jsonl",
        type=Path,
        default=None,
        metavar="PATH",
        help="write machine-readable JSON-lines records (manifest + per-call) to PATH",
    )
    parser.add_argument(
        "--warmup",
        type=int,
        default=3,
        metavar="N",
        help="warmup calls per cell, recorded separately (default: 3)",
    )
    parser.add_argument(
        "--reps",
        type=int,
        default=15,
        metavar="N",
        help="measured repetitions per cell (default: 15)",
    )
    parser.add_argument(
        "--timeout",
        type=float,
        default=120.0,
        metavar="SECONDS",
        help="readiness timeout for each daemon lifecycle state (default: 120)",
    )
    parser.add_argument(
        "--keep-outputs",
        action="store_true",
        help="keep the run-scoped output directory instead of removing it on completion",
    )
    parser.add_argument(
        "--version",
        action="version",
        version=f"%(prog)s {__version__}",
    )
    return parser


def validate_args(parser: argparse.ArgumentParser, args: argparse.Namespace) -> None:
    if args.warmup < 0:
        parser.error("--warmup must be >= 0")
    if args.reps < 1:
        parser.error("--reps must be >= 1")
    if args.timeout <= 0:
        parser.error("--timeout must be > 0")


def main() -> None:
    parser = build_parser()
    args = parser.parse_args()
    validate_args(parser, args)

    from .harness import Harness, HarnessFailure

    harness = Harness(args)
    code = 0
    try:
        asyncio.run(harness.run())
    except KeyboardInterrupt:
        harness.interrupted = True
        code = 130
    except HarnessFailure as error:
        print(f"sophon-bench: error: {error}", file=sys.stderr)
        code = 1
    raise SystemExit(harness.finish(code))


if __name__ == "__main__":
    main()
