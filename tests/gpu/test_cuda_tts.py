"""Real GPU & CPU ONNX tests for VieNeu-TTS v3 Turbo including cancellation and OOM recovery."""

from __future__ import annotations

import os
import sys
import threading
from pathlib import Path
from unittest.mock import patch

REPO_ROOT = Path(__file__).resolve().parents[2]
WORKERS_DIR = REPO_ROOT / "workers"
if str(WORKERS_DIR) not in sys.path:
    sys.path.insert(0, str(WORKERS_DIR))

from tts.engine import VieNeuTtsEngine, inspect_nvml_vram  # noqa: E402


def test_vieneu_tts_cuda_synthesis_cancellation_and_oom_recovery() -> None:
    gpu_ok, gpu_name, total_mb, _, free_mb = inspect_nvml_vram()
    assert gpu_ok is True
    assert gpu_name is not None and "NVIDIA" in gpu_name
    assert total_mb >= 4096 and free_mb > 1000

    models_dir = Path(os.environ["LOCALAPPDATA"]) / "LumaTranslate" / "models" / "tts"
    engine = VieNeuTtsEngine(execution_mode="cuda", models_dir=str(models_dir))
    info = engine.initialize()
    assert info["device"] == "cuda"
    assert info["sample_rate"] == 48000

    chunks = list(engine.stream_synthesize("Xin chào, rất vui được gặp lại bạn."))
    total_samples = sum(int(c.size) for c in chunks)
    assert total_samples > 24000  # > 0.5s of 48kHz audio

    # Pre-set cancellation stops synthesis immediately
    cancel = threading.Event()
    cancel.set()
    cancelled_chunks = list(engine.stream_synthesize("Câu này đã bị hủy ngay lập tức.", cancel))
    assert len(cancelled_chunks) == 0

    # Simulated CUDA OOM falls back to ONNX CPU seamlessly
    orig_run = engine._run_stream
    state = {"raised": False}

    def oom_once(text: str, ev):
        if not state["raised"]:
            state["raised"] = True
            raise RuntimeError("CUDA out of memory while generating audio tokens")
        yield from orig_run(text, ev)

    with patch.object(engine, "_run_stream", side_effect=oom_once):
        recovered_chunks = list(engine.stream_synthesize("Đã khôi phục sang CPU thành công."))

    assert engine.active_device == "cpu"
    assert engine.fallback_reason is not None and "cuda_oom_recovered" in engine.fallback_reason
    assert sum(int(c.size) for c in recovered_chunks) > 24000

    engine.shutdown()
