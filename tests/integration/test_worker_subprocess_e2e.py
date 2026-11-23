"""Subprocess integration test exercising the real framed binary/JSON stdio protocol
across `workers/ocr/worker.py` (PaddleOCR CUDA) and `workers/tts/worker.py` (VieNeu-TTS).
"""

from __future__ import annotations

import os
import subprocess
import sys
import threading
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
WORKERS_DIR = REPO_ROOT / "workers"
TESTS_GPU_DIR = REPO_ROOT / "tests" / "gpu"
for p in (WORKERS_DIR, TESTS_GPU_DIR):
    if str(p) not in sys.path:
        sys.path.insert(0, str(p))

from shared.protocol import FramedWriter, read_frame  # noqa: E402
from smoke_and_benchmark import generate_fixtures  # noqa: E402


def test_real_ocr_and_tts_worker_subprocesses_end_to_end() -> None:
    fixtures = generate_fixtures()
    ocr_py = WORKERS_DIR / "ocr" / ".venv" / "Scripts" / "python.exe"
    tts_py = WORKERS_DIR / "tts" / ".venv" / "Scripts" / "python.exe"
    models_root = Path(os.environ["LOCALAPPDATA"]) / "LumaTranslate" / "models"

    ocr_proc = subprocess.Popen(
        [str(ocr_py), "-u", str(WORKERS_DIR / "ocr" / "worker.py")],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        close_fds=True,
    )
    tts_proc = subprocess.Popen(
        [str(tts_py), "-u", str(WORKERS_DIR / "tts" / "worker.py")],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        close_fds=True,
    )

    def force_kill_on_timeout() -> None:
        for p in (ocr_proc, tts_proc):
            if p.poll() is None:
                p.kill()

    watchdog = threading.Timer(60.0, force_kill_on_timeout)
    watchdog.daemon = True
    watchdog.start()

    try:
        assert ocr_proc.stdin and ocr_proc.stdout
        assert tts_proc.stdin and tts_proc.stdout
        ocr_writer = FramedWriter(ocr_proc.stdin)
        tts_writer = FramedWriter(tts_proc.stdin)

        # 1. Initialize OCR worker on CUDA
        ocr_writer.send(
            {
                "cmd": "init",
                "request_id": 1,
                "preferred_device": "cuda",
                "models_dir": str(models_root / "paddleocr"),
            }
        )
        ocr_ready = read_frame(ocr_proc.stdout)
        assert ocr_ready is not None
        ocr_hdr, _ = ocr_ready
        assert ocr_hdr["ok"] is True, f"OCR init failed: {ocr_hdr}"
        assert ocr_hdr["device"] == "cuda"

        # 2. Recognize Japanese subtitle PNG over raw binary payload
        ja_png_bytes = fixtures["ja"][0].read_bytes()
        ocr_writer.send(
            {
                "cmd": "ocr",
                "request_id": 2,
                "region_id": "region-1",
                "sequence": 42,
                "source_language": "ja",
                "pixel_format": "png",
            },
            ja_png_bytes,
        )
        ocr_out = read_frame(ocr_proc.stdout)
        assert ocr_out is not None
        res_hdr, _ = ocr_out
        assert res_hdr["ok"] is True, f"OCR recognize failed: {res_hdr}"
        assert res_hdr["inference_device"] == "cuda"
        assert "また会えて" in res_hdr["text"]

        # 3. Initialize VieNeu-TTS worker
        tts_writer.send(
            {
                "cmd": "init",
                "request_id": 10,
                "execution_mode": "auto",
                "models_dir": str(models_root / "tts"),
            }
        )
        tts_ready = read_frame(tts_proc.stdout)
        assert tts_ready is not None
        tts_hdr, _ = tts_ready
        assert tts_hdr["ok"] is True, f"TTS init failed: {tts_hdr}"
        assert tts_hdr["sample_rate"] == 48000

        # 4. Stream Vietnamese audio chunks over raw binary f32le frames
        tts_writer.send(
            {
                "cmd": "synthesize",
                "request_id": 11,
                "utterance_id": 1,
                "text": "Rất vui khi được gặp lại bạn.",
            }
        )
        total_audio_bytes = 0
        while True:
            ev = read_frame(tts_proc.stdout)
            assert ev is not None
            ev_hdr, ev_bin = ev
            assert ev_hdr["ok"] is True, f"TTS worker returned error frame: {ev_hdr}"
            if ev_hdr.get("event") == "audio_chunk":
                total_audio_bytes += len(ev_bin)
            elif ev_hdr.get("event") == "done":
                break
        assert total_audio_bytes > 96000  # > 0.5s of 48kHz f32le PCM

        # 5. Clean shutdown of both AI workers
        ocr_writer.send({"cmd": "shutdown", "request_id": 99})
        tts_writer.send({"cmd": "shutdown", "request_id": 100})
        assert ocr_proc.wait(timeout=10) == 0
        assert tts_proc.wait(timeout=10) == 0
    finally:
        watchdog.cancel()
        if ocr_proc.poll() is None:
            ocr_proc.kill()
        if tts_proc.poll() is None:
            tts_proc.kill()
