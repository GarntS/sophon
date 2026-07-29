"""Run orchestration: connect, gate on readiness, measure, report, clean up.

The harness only ever makes ordinary client method calls and property reads;
it never starts, stops, or reconfigures the daemon. Generated speech files
land in a run-scoped output directory that is removed on completion unless
the operator passes ``--keep-outputs``.
"""

from __future__ import annotations

import shutil
import sys
import tempfile
from pathlib import Path

from .baseline import baseline_is_valid, run_baseline
from .client import SophonClient
from .corpus import CorpusError, acquire_corpus
from .discovery import (
    ReadinessError,
    build_manifest,
    discover_config,
    wait_stt_ready,
    wait_tts_ready,
)
from .measure import CellResult
from .report import JsonlWriter, print_report
from .sweeps import run_stt_sweep, run_tts_first_call, run_tts_sweep


class HarnessFailure(Exception):
    """A fatal run error; reported on stderr with a nonzero exit."""


def _note(message: str) -> None:
    print(f"sophon-bench: {message}", file=sys.stderr)


class Harness:
    def __init__(self, args) -> None:
        self.args = args
        self.client = SophonClient()
        self.cells: list[CellResult] = []
        self.interrupted = False
        self.work_dir: Path | None = None
        self._jsonl: JsonlWriter | None = None
        self._manifest: dict | None = None

    async def run(self) -> None:
        args = self.args
        try:
            await self.client.connect()
        except Exception as error:
            raise HarnessFailure(f"could not connect to the session bus: {error}")
        if not self.client.fd_passing:
            _note(
                "Unix FD passing could not be negotiated with the bus; "
                "SpeakToBuffer cells will be skipped"
            )

        # Discovery and readiness gating. STT must become Ready; TTS degrades
        # to a skip notice when it does not.
        try:
            await wait_stt_ready(self.client, args.timeout)
        except ReadinessError as error:
            raise HarnessFailure(str(error))
        tts = await wait_tts_ready(self.client, args.timeout)
        if tts.notice is not None:
            _note(tts.notice)

        # Manifest capture, then the JSONL stream (manifest record first).
        config = await discover_config(self.client)
        self._manifest = build_manifest(
            config, self.client.fd_passing, args.warmup, args.reps
        )
        _note(
            "daemon: engine={engine} model={model} tts_provider={tts_provider} "
            "tts_model={tts_model} capabilities={caps}".format(
                engine=config.engine or "<none>",
                model=config.model or "<none>",
                tts_provider=config.tts_provider or "<none>",
                tts_model=config.tts_model or "<none>",
                caps=",".join(config.tts_capabilities) or "<none>",
            )
        )
        if args.jsonl is not None:
            self._jsonl = JsonlWriter(args.jsonl, self._manifest)

        self.work_dir = Path(tempfile.mkdtemp(prefix="sophon-bench-"))

        try:
            on_record = self._jsonl.write_call if self._jsonl else None

            # Validate an explicitly supplied corpus before making benchmark
            # calls. Fallback acquisition is delayed until after the first TTS
            # call because synthesis itself uses TTS.
            corpus = None
            if args.wav_dir is not None:
                try:
                    corpus = await acquire_corpus(
                        args, self.client, tts.ready, self.work_dir
                    )
                except CorpusError as error:
                    raise HarnessFailure(str(error)) from error

            tts_first_cell = None
            if tts.ready:
                tts_first_cell = await run_tts_first_call(
                    self.client, self.work_dir, self.cells, on_record
                )

            if args.wav_dir is None:
                try:
                    corpus = await acquire_corpus(
                        args, self.client, tts.ready, self.work_dir
                    )
                except CorpusError as error:
                    raise HarnessFailure(str(error)) from error

            await run_baseline(
                self.client,
                tts.ready,
                args.warmup,
                args.reps,
                self.work_dir,
                self.cells,
                on_record,
            )
            if corpus is not None:
                await run_stt_sweep(
                    self.client,
                    corpus,
                    args.warmup,
                    args.reps,
                    self.cells,
                    on_record,
                )
            if tts.ready:
                assert tts_first_cell is not None
                await run_tts_sweep(
                    self.client,
                    self.client.fd_passing,
                    args.warmup,
                    args.reps,
                    self.work_dir,
                    self.cells,
                    tts_first_cell,
                    on_record,
                )
        finally:
            self.client.disconnect()

    def finish(self, code: int) -> int:
        """Print the (possibly partial) report and clean up; return exit code."""
        baseline_cells = [c for c in self.cells if c.name.startswith("baseline/")]
        baseline_valid = baseline_is_valid(baseline_cells) if baseline_cells else True
        if any(cell.records for cell in self.cells):
            print_report(
                self.cells,
                partial=code != 0,
                baseline_valid=baseline_valid,
            )
        if self._jsonl is not None:
            self._jsonl.close()
            _note(f"JSONL records written to {self.args.jsonl}")
        if self.work_dir is not None:
            if self.args.keep_outputs:
                _note(f"run outputs kept in {self.work_dir}")
            else:
                shutil.rmtree(self.work_dir, ignore_errors=True)
        if self.interrupted:
            _note("interrupted; partial results above")
        return code
