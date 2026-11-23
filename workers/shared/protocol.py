"""Framed binary + JSON stdio IPC protocol shared by LumaTranslate AI workers.

Wire format (little-endian):
    [u32 header_json_len][u32 binary_payload_len][utf-8 json bytes][raw binary bytes]

Diagnostics MUST be written exclusively to stderr so stdout remains a clean framed stream.
"""

from __future__ import annotations

import json
import os
import struct
import sys
import threading
from typing import Any, BinaryIO

HEADER_STRUCT = struct.Struct("<II")
MAX_JSON_HEADER_BYTES = 1 * 1024 * 1024      # 1 MiB
MAX_BINARY_PAYLOAD_BYTES = 32 * 1024 * 1024  # 32 MiB


def reserve_clean_ipc_stdout() -> BinaryIO:
    """Capture binary stdout buffer for exclusive IPC framing and route Python sys.stdout to stderr."""
    raw_out = sys.stdout.buffer
    if os.name == "nt":
        import msvcrt

        msvcrt.setmode(raw_out.fileno(), os.O_BINARY)
        msvcrt.setmode(sys.stdin.buffer.fileno(), os.O_BINARY)
    sys.stdout = sys.stderr
    return raw_out


class ProtocolError(RuntimeError):
    """Raised when framed worker IPC violates size bounds or stream integrity."""


def read_exact(stream: BinaryIO, count: int) -> bytes:
    """Read exactly ``count`` bytes from ``stream``, or return b'' on clean EOF at boundary."""
    if count == 0:
        return b""
    chunks = bytearray()
    remaining = count
    while remaining > 0:
        chunk = stream.read(remaining)
        if not chunk:
            if not chunks:
                return b""
            raise ProtocolError(
                f"Unexpected EOF while reading frame ({len(chunks)}/{count} bytes read)"
            )
        chunks.extend(chunk)
        remaining -= len(chunk)
    return bytes(chunks)


def read_frame(stream: BinaryIO | None = None) -> tuple[dict[str, Any], bytes] | None:
    """Read one framed message from ``stream`` (defaults to ``sys.stdin.buffer``).

    Returns ``None`` on clean EOF before a frame header.
    """
    src = stream if stream is not None else sys.stdin.buffer
    prefix = read_exact(src, HEADER_STRUCT.size)
    if not prefix:
        return None
    if len(prefix) != HEADER_STRUCT.size:
        raise ProtocolError("Incomplete 8-byte frame prefix")

    json_len, payload_len = HEADER_STRUCT.unpack(prefix)
    if json_len == 0 or json_len > MAX_JSON_HEADER_BYTES:
        raise ProtocolError(f"Invalid JSON header length: {json_len}")
    if payload_len > MAX_BINARY_PAYLOAD_BYTES:
        raise ProtocolError(f"Binary payload exceeds limit: {payload_len}")

    json_bytes = read_exact(src, json_len)
    if len(json_bytes) != json_len:
        raise ProtocolError("Truncated JSON header")

    payload = read_exact(src, payload_len) if payload_len > 0 else b""
    if len(payload) != payload_len:
        raise ProtocolError("Truncated binary payload")

    try:
        header = json.loads(json_bytes.decode("utf-8"))
    except Exception as exc:
        raise ProtocolError(f"Invalid UTF-8 JSON frame header: {exc}") from exc

    if not isinstance(header, dict):
        raise ProtocolError("Frame JSON header must be an object")

    return header, payload


class FramedWriter:
    """Thread-safe framed message writer over a binary output stream."""

    def __init__(self, stream: BinaryIO | None = None) -> None:
        self._stream = stream if stream is not None else sys.stdout.buffer
        self._lock = threading.Lock()

    def send(self, header: dict[str, Any], payload: bytes | memoryview = b"") -> None:
        json_bytes = json.dumps(header, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
        json_len = len(json_bytes)
        payload_len = len(payload)
        if json_len > MAX_JSON_HEADER_BYTES:
            raise ProtocolError(f"Outgoing JSON header too large: {json_len}")
        if payload_len > MAX_BINARY_PAYLOAD_BYTES:
            raise ProtocolError(f"Outgoing binary payload too large: {payload_len}")

        prefix = HEADER_STRUCT.pack(json_len, payload_len)
        with self._lock:
            self._stream.write(prefix)
            self._stream.write(json_bytes)
            if payload_len > 0:
                self._stream.write(payload)
            self._stream.flush()


def log_stderr(worker_name: str, message: str) -> None:
    """Write structured diagnostic line to stderr without polluting stdout IPC."""
    sys.stderr.write(f"[{worker_name}] {message}\n")
    sys.stderr.flush()
