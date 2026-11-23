"""Real GPU + PaddleOCR PP-OCRv5 Mobile tests including VRAM inspection & OOM recovery."""

from __future__ import annotations

import os
import sys
from pathlib import Path
from unittest.mock import patch

import numpy as np

REPO_ROOT = Path(__file__).resolve().parents[2]
WORKERS_DIR = REPO_ROOT / "workers"
TESTS_GPU_DIR = REPO_ROOT / "tests" / "gpu"
for p in (WORKERS_DIR, TESTS_GPU_DIR):
    if str(p) not in sys.path:
        sys.path.insert(0, str(p))

from ocr.engine import PaddleOcrEngine, inspect_nvml_gpu  # noqa: E402
from smoke_and_benchmark import generate_fixtures  # noqa: E402


def test_nvml_detects_nvidia_gpu_and_vram() -> None:
    snap = inspect_nvml_gpu()
    assert snap.available is True
    assert snap.name is not None and "NVIDIA" in snap.name
    assert snap.vram_total_mb >= 4096
    assert snap.vram_free_mb > 0


def test_paddleocr_real_cuda_multilingual_inference_and_cleanup() -> None:
    fixtures = generate_fixtures()
    models_dir = Path(os.environ["LOCALAPPDATA"]) / "LumaTranslate" / "models" / "paddleocr"
    engine = PaddleOcrEngine(preferred_device="cuda", models_dir=str(models_dir))
    info = engine.initialize()

    assert info["device"] == "cuda"
    assert info["fallback_reason"] is None

    for key in ("en", "ja", "zh", "ko"):
        fpath, expected, lang = fixtures[key]
        bgr = engine.decode_image(fpath.read_bytes(), pixel_format="png")
        out = engine.recognize(bgr, region_id=f"test-{key}", sequence=1, source_language=lang)
        assert out["inference_device"] == "cuda"
        assert out["confidence"] > 0.85
        # Verify core phrase recognized accurately
        assert expected[:4] in out["text"]

    engine.shutdown()


def test_paddleocr_recovers_to_cpu_on_simulated_cuda_oom() -> None:
    fixtures = generate_fixtures()
    models_dir = Path(os.environ["LOCALAPPDATA"]) / "LumaTranslate" / "models" / "paddleocr"
    engine = PaddleOcrEngine(preferred_device="cuda", models_dir=str(models_dir))
    info = engine.initialize()
    assert info["device"] == "cuda"

    fpath, expected, _ = fixtures["en"]
    bgr = engine.decode_image(fpath.read_bytes(), pixel_format="png")

    real_get = engine._get_or_create_pipeline
    calls = {"count": 0}

    class OomOnceWrapper:
        def predict(self, _img: np.ndarray):
            raise RuntimeError("ResourceExhaustedError: CUDA out of memory")

    def side_effect(model_name: str):
        calls["count"] += 1
        if calls["count"] == 1:
            return OomOnceWrapper()
        return real_get(model_name)

    with patch.object(engine, "_get_or_create_pipeline", side_effect=side_effect):
        recovered = engine.recognize(bgr, region_id="oom-reg", sequence=9, source_language="en")

    assert recovered["inference_device"] == "cpu"
    assert engine.active_device == "cpu"
    assert engine.fallback_reason is not None and "cuda_oom_recovered" in engine.fallback_reason
    assert expected[:5] in recovered["text"]
    engine.shutdown()
