"""Async D-Bus client core for the sophon daemon.

Wraps the ``com.garntresearch.sophon`` session-bus object with typed method
calls and property reads, and maps the daemon's stable error names onto
structured harness exceptions so rejection calls can be verified by type.
"""

from __future__ import annotations

import asyncio
from typing import Any

from dbus_next import BusType, Message, MessageType
from dbus_next.aio import MessageBus
from dbus_next.errors import AuthError
from dbus_next.signature import Variant

BUS_NAME = "com.garntresearch.sophon"
OBJECT_PATH = "/com/garntresearch/sophon"
INTERFACE = "com.garntresearch.sophon"
ERROR_PREFIX = "com.garntresearch.sophon."

# Ceiling for a single D-Bus call. A client-side timeout does not cancel work
# the daemon already accepted, so this only guards against a wedged daemon.
CALL_TIMEOUT_S = 1800.0

PROPERTIES_INTERFACE = "org.freedesktop.DBus.Properties"


class DaemonError(Exception):
    """An error reply from the daemon (or the bus transport)."""

    def __init__(self, name: str, message: str):
        super().__init__(f"{name}: {message}")
        self.name = name
        self.message = message


class NotReadyError(DaemonError):
    pass


class InvalidOptionsError(DaemonError):
    pass


class InvalidAudioError(DaemonError):
    pass


class ModelUnavailableError(DaemonError):
    pass


class ResourceLimitError(DaemonError):
    pass


class TranscriptionFailedError(DaemonError):
    pass


class InvalidTtsOptionsError(DaemonError):
    pass


class InvalidReferenceAudioError(DaemonError):
    pass


class UnsupportedCapabilityError(DaemonError):
    pass


class OutputExistsError(DaemonError):
    pass


class OutputFailedError(DaemonError):
    pass


class SynthesisFailedError(DaemonError):
    pass


class PlaybackFailedError(DaemonError):
    pass


_ERROR_MAP = {
    "NotReady": NotReadyError,
    "InvalidOptions": InvalidOptionsError,
    "InvalidAudio": InvalidAudioError,
    "ModelUnavailable": ModelUnavailableError,
    "ResourceLimit": ResourceLimitError,
    "TranscriptionFailed": TranscriptionFailedError,
    "InvalidTtsOptions": InvalidTtsOptionsError,
    "InvalidReferenceAudio": InvalidReferenceAudioError,
    "UnsupportedCapability": UnsupportedCapabilityError,
    "OutputExists": OutputExistsError,
    "OutputFailed": OutputFailedError,
    "SynthesisFailed": SynthesisFailedError,
    "PlaybackFailed": PlaybackFailedError,
}


def translate_error(name: str, message: str) -> DaemonError:
    """Map a D-Bus error name to a structured harness exception."""
    suffix = name[len(ERROR_PREFIX) :] if name.startswith(ERROR_PREFIX) else name
    return _ERROR_MAP.get(suffix, DaemonError)(suffix, message)


def is_daemon_error_name(name: str) -> bool:
    """True when the error name belongs to the daemon's stable namespace."""
    return name.startswith(ERROR_PREFIX)


class SophonClient:
    """Typed async wrapper around the daemon's session-bus object."""

    def __init__(self) -> None:
        self._bus: MessageBus | None = None
        self.fd_passing = False

    async def connect(self) -> None:
        """Connect to the session bus, negotiating Unix FD passing.

        Records whether descriptor transfer is available in ``fd_passing``;
        when the bus refuses FD negotiation the client falls back to a plain
        connection and buffer-mode calls must be skipped.
        """
        try:
            self._bus = await MessageBus(
                bus_type=BusType.SESSION, negotiate_unix_fd=True
            ).connect()
            self.fd_passing = True
        except AuthError:
            self._bus = await MessageBus(bus_type=BusType.SESSION).connect()
            self.fd_passing = False

    def disconnect(self) -> None:
        if self._bus is not None:
            self._bus.disconnect()
            self._bus = None

    async def _call(self, message: Message) -> Message:
        assert self._bus is not None, "client is not connected"
        reply = await asyncio.wait_for(self._bus.call(message), timeout=CALL_TIMEOUT_S)
        if reply is None or reply.message_type == MessageType.ERROR:
            if reply is None:
                raise DaemonError("NoReply", "no reply from daemon")
            name = str(reply.error_name or "unknown")
            text = str(reply.body[0]) if reply.body else ""
            raise translate_error(name, text)
        return reply

    async def _method(self, member: str, signature: str, body: list) -> Message:
        return await self._call(
            Message(
                destination=BUS_NAME,
                path=OBJECT_PATH,
                interface=INTERFACE,
                member=member,
                signature=signature,
                body=body,
            )
        )

    # -- typed method wrappers ------------------------------------------------

    async def transcribe_file(self, path: str, options: dict | None = None) -> str:
        reply = await self._method(
            "TranscribeFile", "sa{sv}", [path, options or {}]
        )
        return str(reply.body[0])

    async def speak_to_file(
        self, text: str, path: str, options: dict | None = None
    ) -> int:
        reply = await self._method(
            "SpeakToFile", "ssa{sv}", [text, path, options or {}]
        )
        return int(reply.body[0])

    async def speak_to_buffer(self, text: str, options: dict | None = None) -> tuple[int, int]:
        """Return ``(fd, size_bytes)`` for the sealed memfd WAV.

        The caller owns the returned descriptor. Raw replies carry the fd
        index in the body; resolve it against the transferred descriptors.
        """
        reply = await self._method("SpeakToBuffer", "sa{sv}", [text, options or {}])
        fd_value, size = int(reply.body[0]), int(reply.body[1])
        if reply.unix_fds and 0 <= fd_value < len(reply.unix_fds):
            fd_value = int(reply.unix_fds[fd_value])
        return fd_value, size

    # -- typed property reads -------------------------------------------------

    async def get_property(self, name: str) -> Any:
        reply = await self._call(
            Message(
                destination=BUS_NAME,
                path=OBJECT_PATH,
                interface=PROPERTIES_INTERFACE,
                member="Get",
                signature="ss",
                body=[INTERFACE, name],
            )
        )
        value = reply.body[0]
        return value.value if isinstance(value, Variant) else value

    async def state(self) -> str:
        return str(await self.get_property("State"))

    async def tts_state(self) -> str:
        return str(await self.get_property("TtsState"))

    async def active_provider(self) -> str:
        return str(await self.get_property("ActiveProvider"))

    async def active_model(self) -> str:
        return str(await self.get_property("ActiveModel"))

    async def active_tts_provider(self) -> str:
        return str(await self.get_property("ActiveTtsProvider"))

    async def active_tts_model(self) -> str:
        return str(await self.get_property("ActiveTtsModel"))

    async def tts_capabilities(self) -> list[str]:
        return [str(v) for v in await self.get_property("TtsCapabilities")]

    async def last_error(self) -> str:
        return str(await self.get_property("LastError"))

    async def tts_last_error(self) -> str:
        return str(await self.get_property("TtsLastError"))
