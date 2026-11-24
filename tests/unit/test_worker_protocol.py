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
from ocr.engine import merge_subtitle_polygons  # noqa: E402
from tts.engine import sanitize_tts_text  # noqa: E402


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


def test_merge_subtitle_polygons_joins_same_line_words_and_wrapped_captions() -> None:
    # Simulate PaddleOCR returning separate word polygons across a 2-line English caption
    boxes = [
        (24.0, 100.0, 165.0, 22.0, "basic", 0.99),
        (25.5, 175.0, 225.0, 22.0, "kind", 0.98),
        (23.0, 235.0, 260.0, 22.0, "of", 0.99),
        (26.0, 270.0, 380.0, 22.0, "questions", 0.97),
        (54.0, 120.0, 165.0, 22.0, "that", 0.99),
        (55.0, 175.0, 205.0, 22.0, "we", 0.99),
        (53.5, 215.0, 255.0, 22.0, "are", 0.98),
    ]
    merged, conf = merge_subtitle_polygons(boxes)
    assert merged == "basic kind of questions that we are"
    assert conf > 0.95

    # CJK tokens on the same line should concatenate without inserting ASCII spaces
    cjk_boxes = [
        (30.0, 50.0, 150.0, 26.0, "また会えて", 0.99),
        (31.0, 155.0, 260.0, 26.0, "嬉しい。", 0.98),
    ]
    merged_cjk, _ = merge_subtitle_polygons(cjk_boxes)
    assert merged_cjk == "また会えて嬉しい。"


def test_sanitize_tts_text_normalizes_newlines_and_box_noise() -> None:
    assert sanitize_tts_text("Xin chào\n□\nrất vui   được gặp bạn.") == "Xin chào rất vui được gặp bạn."
