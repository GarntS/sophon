from __future__ import annotations

import asyncio
import contextlib
import io
import json
import tempfile
import unittest
import wave
from pathlib import Path

from sophon_bench.baseline import baseline_is_valid
from sophon_bench.corpus import CorpusError, load_wav_dir
from sophon_bench.measure import (
    KIND_FIRST,
    KIND_MEASURED,
    KIND_WARMUP,
    CallRecord,
    CellResult,
    run_cell,
)
from sophon_bench.report import JsonlWriter, compute_stats


class MeasurementTests(unittest.IsolatedAsyncioTestCase):
    async def test_first_warmups_and_measured_calls_are_separate(self) -> None:
        calls = 0

        async def call() -> int:
            nonlocal calls
            calls += 1
            await asyncio.sleep(0)
            return calls

        with contextlib.redirect_stderr(io.StringIO()):
            cell = await run_cell(
                "test/cell",
                {},
                warmup=2,
                reps=3,
                call_fn=call,
                rtf_fn=lambda latency, result: float(result),
                record_first=True,
            )

        self.assertEqual(calls, 6)
        self.assertEqual(
            [record.kind for record in cell.records],
            [KIND_FIRST, KIND_WARMUP, KIND_WARMUP] + [KIND_MEASURED] * 3,
        )
        self.assertEqual([record.first for record in cell.records], [True] + [False] * 5)
        self.assertEqual(len(cell.measured), 3)
        self.assertEqual(compute_stats(cell).n, 3)

    async def test_jsonl_contains_only_manifest_and_measured_calls(self) -> None:
        records: list[CallRecord] = []
        with contextlib.redirect_stderr(io.StringIO()):
            await run_cell(
                "test/cell",
                {"axis": "value"},
                warmup=2,
                reps=3,
                call_fn=lambda: asyncio.sleep(0, result=None),
                rtf_fn=lambda latency, result: None,
                on_record=records.append,
                record_first=True,
            )

        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "report.jsonl"
            writer = JsonlWriter(path, {"type": "manifest"})
            for record in records:
                writer.write_call(record)
            writer.close()
            rows = [json.loads(line) for line in path.read_text().splitlines()]

        self.assertEqual(rows[0], {"type": "manifest"})
        self.assertEqual(len(rows), 4)
        self.assertTrue(all(row["kind"] == KIND_MEASURED for row in rows[1:]))


class CorpusTests(unittest.TestCase):
    @staticmethod
    def _write_wav(path: Path, duration_s: float) -> None:
        with wave.open(str(path), "wb") as wav:
            wav.setnchannels(1)
            wav.setsampwidth(2)
            wav.setframerate(16_000)
            wav.writeframes(b"\0\0" * int(16_000 * duration_s))

    def test_user_corpus_requires_three_duration_buckets(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self._write_wav(root / "one.wav", 1.0)
            with self.assertRaisesRegex(CorpusError, "at least 3 distinct buckets"):
                load_wav_dir(root)

    def test_user_corpus_accepts_three_duration_buckets(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for duration in (1.0, 3.0, 10.0):
                self._write_wav(root / f"{duration:g}.wav", duration)
            self.assertEqual(len(load_wav_dir(root)), 3)


class BaselineTests(unittest.TestCase):
    @staticmethod
    def _record(*, error: str | None = None, invalid: bool = False) -> CallRecord:
        return CallRecord(
            cell="baseline/test",
            kind=KIND_MEASURED,
            first=False,
            latency_s=0.001,
            rtf=None,
            axes={},
            error=error,
            invalid=invalid,
        )

    def test_wrong_error_type_invalidates_baseline(self) -> None:
        cell = CellResult("baseline/test", {}, [self._record(error="NotReady")])
        self.assertFalse(baseline_is_valid([cell]))

    def test_unexpected_success_invalidates_baseline(self) -> None:
        cell = CellResult("baseline/test", {}, [self._record(invalid=True)])
        self.assertFalse(baseline_is_valid([cell]))

    def test_expected_rejections_leave_baseline_valid(self) -> None:
        cell = CellResult("baseline/test", {}, [self._record()])
        self.assertTrue(baseline_is_valid([cell]))


if __name__ == "__main__":
    unittest.main()
