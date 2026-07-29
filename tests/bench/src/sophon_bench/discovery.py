"""Daemon discovery, readiness gating, and run-manifest capture."""

from __future__ import annotations

import asyncio
import platform
import time
from dataclasses import dataclass, field
from datetime import datetime, timezone
from typing import Awaitable, Callable

from . import __version__
from .client import DaemonError, SophonClient, is_daemon_error_name

POLL_INTERVAL_S = 0.5


class ReadinessError(Exception):
    """A lifecycle state never reached Ready (or reached Failed)."""

    def __init__(self, label: str, state: str, last_error: str, reason: str):
        self.label = label
        self.state = state
        self.last_error = last_error
        detail = f", last error: {last_error}" if last_error else ""
        super().__init__(f"{label} {reason} (state: {state}{detail})")


async def _poll_ready(
    label: str,
    get_state: Callable[[], Awaitable[str]],
    get_last_error: Callable[[], Awaitable[str]],
    timeout_s: float,
) -> None:
    """Poll a lifecycle state property until Ready.

    Raises ReadinessError on Failed or timeout, naming the observed state and
    the daemon's reported last error.
    """
    deadline = time.monotonic() + timeout_s
    state = "<unread>"
    while True:
        try:
            state = await get_state()
        except DaemonError as error:
            if not is_daemon_error_name(error.name):
                # Transport-level failure (e.g. the daemon name is not owned).
                raise ReadinessError(label, "<absent>", "", str(error)) from error
            raise
        if state == "Ready":
            return
        if state == "Failed":
            raise ReadinessError(
                label, state, await get_last_error(), "reports Failed"
            )
        if time.monotonic() >= deadline:
            raise ReadinessError(
                label,
                state,
                await get_last_error(),
                f"did not become Ready within {timeout_s:g}s",
            )
        await asyncio.sleep(POLL_INTERVAL_S)


async def wait_stt_ready(client: SophonClient, timeout_s: float) -> None:
    """Wait for the STT lifecycle to reach Ready; failure aborts the run."""
    await _poll_ready("STT", client.state, client.last_error, timeout_s)


@dataclass
class TtsReadiness:
    ready: bool
    notice: str | None = None


async def wait_tts_ready(client: SophonClient, timeout_s: float) -> TtsReadiness:
    """Wait for the TTS lifecycle, degrading to a skip notice when unready.

    Unlike STT, an unready TTS does not abort the run; the TTS sweep is
    skipped with a clear notice instead.
    """
    try:
        await _poll_ready("TTS", client.tts_state, client.tts_last_error, timeout_s)
    except ReadinessError as error:
        return TtsReadiness(
            ready=False,
            notice=f"TTS sweep skipped: {error}",
        )
    return TtsReadiness(ready=True)


def _cpu_model() -> str:
    try:
        with open("/proc/cpuinfo", encoding="utf-8") as handle:
            for line in handle:
                if line.startswith("model name"):
                    return line.split(":", 1)[1].strip()
    except OSError:
        pass
    return platform.processor() or "unknown"


@dataclass
class DaemonConfig:
    """The discovered configuration of the running daemon."""

    engine: str
    model: str
    tts_provider: str
    tts_model: str
    tts_capabilities: list[str] = field(default_factory=list)


async def discover_config(client: SophonClient) -> DaemonConfig:
    """Read the daemon's published lifecycle properties."""
    return DaemonConfig(
        engine=await client.active_engine(),
        model=await client.active_model(),
        tts_provider=await client.active_tts_provider(),
        tts_model=await client.active_tts_model(),
        tts_capabilities=await client.tts_capabilities(),
    )


def build_manifest(
    daemon: DaemonConfig,
    fd_passing: bool,
    warmup: int,
    reps: int,
) -> dict:
    """Assemble the JSONL manifest record for this run."""
    uname = platform.uname()
    return {
        "type": "manifest",
        "harness": "sophon-bench",
        "version": __version__,
        "timestamp": datetime.now(timezone.utc).isoformat(),
        "platform": {
            "system": uname.system,
            "release": uname.release,
            "machine": uname.machine,
            "python": platform.python_version(),
        },
        "cpu_model": _cpu_model(),
        "daemon": {
            "engine": daemon.engine,
            "model": daemon.model,
            "tts_provider": daemon.tts_provider,
            "tts_model": daemon.tts_model,
            "tts_capabilities": list(daemon.tts_capabilities),
        },
        "fd_passing": fd_passing,
        "parameters": {"warmup": warmup, "reps": reps},
    }
