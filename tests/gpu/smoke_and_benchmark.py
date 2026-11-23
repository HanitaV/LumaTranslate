"""Generate multilingual subtitle fixtures and verify real CUDA OCR & VieNeu-TTS execution."""

from __future__ import annotations

import inspect
import json
import os
import sys
import time
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
FIXTURES_DIR = REPO_ROOT / "tests" / "fixtures"
WORKERS_DIR = REPO_ROOT / "workers"
if str(WORKERS_DIR) not in sys.path:
    sys.path.insert(0, str(WORKERS_DIR))


def generate_fixtures() -> dict[str, tuple[Path, str, str]]:
    from PIL import Image, ImageDraw, ImageFont

    FIXTURES_DIR.mkdir(parents=True, exist_ok=True)
    win_fonts = Path(os.environ.get("WINDIR", r"C:\Windows")) / "Fonts"

    samples = {
        "en": ("Hello! Welcome back to the adventure.", ["segoeui.ttf", "arial.ttf"], "en"),
        "ja": ("また会えて嬉しい。", ["msgothic.ttc", "YuGothM.ttc", "meiryo.ttc"], "ja"),
        "zh": ("很高兴再次见到你。", ["msyh.ttc", "simhei.ttf"], "zh"),
        "ko": ("다시 만나서 반가워요.", ["malgun.ttf", "gulim.ttc"], "ko"),
    }

    created: dict[str, tuple[Path, str, str]] = {}
    for key, (text, font_candidates, lang) in samples.items():
        font = None
        for fname in font_candidates:
            fpath = win_fonts / fname
            if fpath.exists():
                font = ImageFont.truetype(str(fpath), 36)
                break
        if font is None:
            font = ImageFont.load_default()

        img = Image.new("RGB", (900, 110), color=(18, 22, 28))
        draw = ImageDraw.Draw(img)
        draw.text((32, 30), text, fill=(248, 250, 252), font=font)
        out_path = FIXTURES_DIR / f"subtitle_{key}.png"
        img.save(out_path)
        created[key] = (out_path, text, lang)

    return created


def run_ocr_smoke() -> dict:
    from ocr.engine import PaddleOcrEngine

    fixtures = generate_fixtures()
    models_root = Path(os.environ["LOCALAPPDATA"]) / "LumaTranslate" / "models" / "paddleocr"
    engine = PaddleOcrEngine(preferred_device="cuda", models_dir=str(models_root))
    init_info = engine.initialize()

    results = {}
    for key, (path, expected, lang) in fixtures.items():
        payload = path.read_bytes()
        bgr = engine.decode_image(payload, pixel_format="png")
        # Warm run + measured run
        _ = engine.recognize(bgr, region_id=f"reg-{key}", sequence=1, source_language=lang)
        out = engine.recognize(bgr, region_id=f"reg-{key}", sequence=2, source_language=lang)
        results[key] = {
            "expected": expected,
            "recognized": out["text"],
            "confidence": out["confidence"],
            "latency_ms": out["latency_ms"],
            "device": out["inference_device"],
        }

    engine.shutdown()
    return {"init": init_info, "samples": results}


def run_tts_smoke() -> dict:
    import vieneu
    from vieneu import Vieneu
    from tts.engine import VieNeuTtsEngine

    sig = str(inspect.signature(Vieneu.__init__))
    models_root = Path(os.environ["LOCALAPPDATA"]) / "LumaTranslate" / "models" / "tts"

    sentence = "Rất vui khi được gặp lại bạn trong chuyến phiêu lưu này."
    bench = {}

    for mode in ("cuda", "cpu"):
        engine = VieNeuTtsEngine(execution_mode=mode, models_dir=str(models_root))
        init_info = engine.initialize()
        # Measured streaming run
        t0 = time.perf_counter()
        first_chunk_ms = None
        total_samples = 0
        for chunk in engine.stream_synthesize(sentence):
            if first_chunk_ms is None:
                first_chunk_ms = round((time.perf_counter() - t0) * 1000.0, 2)
            total_samples += int(chunk.size)
        total_ms = round((time.perf_counter() - t0) * 1000.0, 2)
        duration_sec = round(total_samples / float(engine.sample_rate), 2)
        engine.shutdown()
        bench[mode] = {
            "init": init_info,
            "first_chunk_ms": first_chunk_ms,
            "total_ms": total_ms,
            "total_samples": total_samples,
            "audio_duration_sec": duration_sec,
        }

    return {"vieneu_version": getattr(vieneu, "__version__", "unknown"), "init_sig": sig, "benchmarks": bench}


if __name__ == "__main__":
    mode = sys.argv[1] if len(sys.argv) > 1 else "all"
    if mode == "ocr":
        print(json.dumps(run_ocr_smoke(), ensure_ascii=False, indent=2))
    elif mode == "tts":
        print(json.dumps(run_tts_smoke(), ensure_ascii=False, indent=2))
    else:
        print("Specify 'ocr' or 'tts'")
