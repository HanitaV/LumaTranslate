"""Unit tests for the framed binary + JSON stdio worker IPC protocol."""

from __future__ import annotations

import io
import struct
import sys
from pathlib import Path

import pytest

WORKERS_DIR = Path(__file__).resolve().parents[2] / "workers"
if str(WORKERS_DIR) not in sys.path:
    sys.path.insert(0, str(WORKERS_DIR))

from shared.protocol import (  # noqa: E402
    MAX_BINARY_PAYLOAD_BYTES,
    FramedWriter,
    ProtocolError,
    read_frame,
)


def test_framed_roundtrip_preserves_utf8_header_and_raw_binary_bytes() -> None:
    buf = io.BytesIO()
    writer = FramedWriter(buf)
    raw_pixels = bytes(range(256)) * 16
    writer.send({"cmd": "ocr", "region_id": "vùng-1", "sequence": 12}, raw_pixels)

    buf.seek(0)
    parsed = read_frame(buf)
    assert parsed is not None
    header, payload = parsed
    assert header["cmd"] == "ocr"
    assert header["region_id"] == "vùng-1"
    assert header["sequence"] == 12
    assert payload == raw_pixels


def test_clean_eof_returns_none() -> None:
    buf = io.BytesIO(b"")
    assert read_frame(buf) is None


def test_rejects_oversized_binary_payload_length() -> None:
    bad = struct.pack("<II", 16, MAX_BINARY_PAYLOAD_BYTES + 1)
    buf = io.BytesIO(bad)
    with pytest.raises(ProtocolError):
        read_frame(buf)
