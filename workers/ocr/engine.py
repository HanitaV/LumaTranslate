"""CUDA-first PaddleOCR PP-OCRv5 Mobile engine with NVML inspection and CPU fallback."""

from __future__ import annotations

import gc
import os
import time
from dataclasses import dataclass
from typing import Any

# Disable network hoster connectivity check during cold start / offline use,
# and disable oneDNN PIR attribute conversion bug on Windows CPU fallback in Paddle 3.3.x.
os.environ.setdefault("PADDLE_PDX_DISABLE_MODEL_SOURCE_CHECK", "True")
os.environ.setdefault("FLAGS_use_mkldnn", "0")

import cv2
import numpy as np


@dataclass
class GpuSnapshot:
    available: bool
    name: str | None
    driver_version: str | None
    vram_total_mb: int
    vram_used_mb: int
    vram_free_mb: int
    gpu_utilization_pct: int | None


def inspect_nvml_gpu(device_index: int = 0) -> GpuSnapshot:
    """Inspect NVIDIA GPU telemetry via NVML without allocating a CUDA context."""
    try:
        import pynvml

        pynvml.nvmlInit()
        try:
            count = pynvml.nvmlDeviceGetCount()
            if count <= device_index:
                return GpuSnapshot(False, None, None, 0, 0, 0, None)
            handle = pynvml.nvmlDeviceGetHandleByIndex(device_index)
            raw_name = pynvml.nvmlDeviceGetName(handle)
            name = raw_name.decode("utf-8") if isinstance(raw_name, bytes) else str(raw_name)
            raw_drv = pynvml.nvmlSystemGetDriverVersion()
            driver = raw_drv.decode("utf-8") if isinstance(raw_drv, bytes) else str(raw_drv)
            mem = pynvml.nvmlDeviceGetMemoryInfo(handle)
            try:
                util = int(pynvml.nvmlDeviceGetUtilizationRates(handle).gpu)
            except Exception:
                util = None
            return GpuSnapshot(
                available=True,
                name=name,
                driver_version=driver,
                vram_total_mb=int(mem.total // (1024 * 1024)),
                vram_used_mb=int(mem.used // (1024 * 1024)),
                vram_free_mb=int(mem.free // (1024 * 1024)),
                gpu_utilization_pct=util,
            )
        finally:
            try:
                pynvml.nvmlShutdown()
            except Exception:
                pass
    except Exception:
        return GpuSnapshot(False, None, None, 0, 0, 0, None)


def _is_cjk_char(ch: str) -> bool:
    code = ord(ch)
    return (
        0x3000 <= code <= 0x30FF  # CJK symbols, Hiragana, Katakana
        or 0x31F0 <= code <= 0x31FF  # Katakana phonetic extensions
        or 0x3400 <= code <= 0x4DBF  # CJK Ext A
        or 0x4E00 <= code <= 0x9FFF  # CJK Unified Ideographs
        or 0xF900 <= code <= 0xFAFF  # CJK Compatibility Ideographs
        or 0xFF00 <= code <= 0xFFEF  # Fullwidth forms
    )


def _join_subtitle_tokens(left: str, right: str) -> str:
    left_s = left.rstrip()
    right_s = right.lstrip()
    if not left_s:
        return right_s
    if not right_s:
        return left_s
    if _is_cjk_char(left_s[-1]) and _is_cjk_char(right_s[0]):
        return f"{left_s}{right_s}"
    return f"{left_s} {right_s}"


def merge_subtitle_polygons(
    boxes: list[tuple[float, float, float, float, str, float]],
) -> tuple[str, float]:
    """Merge detected word/line boxes (cy, min_x, max_x, box_h, text, score) into clean subtitle text.

    Groups word polygons on the same visual line left-to-right with proper spacing, and joins
    wrapped subtitle lines into coherent sentences so cloud translation and TTS preserve context.
    """
    if not boxes:
        return "", 0.0

    sorted_by_y = sorted(boxes, key=lambda b: b[0])
    rows: list[list[tuple[float, float, float, float, str, float]]] = []
    for item in sorted_by_y:
        cy, _, _, box_h, _, _ = item
        placed = False
        for row in rows:
            row_cy = sum(r[0] for r in row) / len(row)
            row_h = sum(r[3] for r in row) / len(row)
            threshold = max(14.0, 0.60 * max(box_h, row_h))
            if abs(cy - row_cy) <= threshold:
                row.append(item)
                placed = True
                break
        if not placed:
            rows.append([item])

    row_entries: list[tuple[float, float, str]] = []
    for row in rows:
        row.sort(key=lambda b: b[1])
        row_cy = sum(r[0] for r in row) / len(row)
        row_h = max(sum(r[3] for r in row) / len(row), 12.0)
        merged_line = ""
        for token in row:
            merged_line = _join_subtitle_tokens(merged_line, token[4])
        if merged_line:
            row_entries.append((row_cy, row_h, merged_line))

    if not row_entries:
        return "", 0.0

    combined = row_entries[0][2]
    prev_cy, prev_h, _ = row_entries[0]
    for cur_cy, cur_h, cur_text in row_entries[1:]:
        avg_h = 0.5 * (prev_h + cur_h)
        vertical_gap = cur_cy - prev_cy
        if vertical_gap > 2.4 * avg_h:
            combined = f"{combined}\n{cur_text}"
        else:
            combined = _join_subtitle_tokens(combined, cur_text)
        prev_cy, prev_h = cur_cy, cur_h

    avg_conf = round(sum(b[5] for b in boxes) / len(boxes), 4)
    return combined.strip(), avg_conf
def map_language_to_rec_model(source_language: str | None) -> tuple[str, str]:
    """Map requested source language code to the validated PP-OCRv5 Mobile recognition model.

    PP-OCRv5_mobile_rec natively supports English, Japanese, Simplified Chinese,
    Traditional Chinese, and Pinyin in a single mobile model.
    """
    lang = (source_language or "auto").strip().lower()
    if lang in ("ko", "kor", "korean"):
        return "korean_PP-OCRv5_mobile_rec", "ko"
    if lang in ("latin", "fr", "de", "es", "it", "pt"):
        return "latin_PP-OCRv5_mobile_rec", lang
    if lang in ("ja", "jp", "japan", "japanese"):
        return "PP-OCRv5_mobile_rec", "ja"
    if lang in ("zh", "zh-cn", "zh-hans", "ch", "chinese"):
        return "PP-OCRv5_mobile_rec", "zh"
    if lang in ("zh-tw", "zh-hant", "chinese_cht"):
        return "PP-OCRv5_mobile_rec", "zh-Hant"
    if lang in ("en", "eng", "english"):
        return "PP-OCRv5_mobile_rec", "en"
    return "PP-OCRv5_mobile_rec", "auto"


class PaddleOcrEngine:
    """Persistent PP-OCRv5 Mobile engine managing GPU/CPU selection and OOM recovery."""

    MIN_FREE_VRAM_FOR_CUDA_MB = 650

    def __init__(
        self,
        preferred_device: str = "cuda",
        models_dir: str | None = None,
        min_free_vram_mb: int = MIN_FREE_VRAM_FOR_CUDA_MB,
    ) -> None:
        if models_dir:
            os.makedirs(models_dir, exist_ok=True)
            os.environ["PADDLE_PDX_CACHE_HOME"] = models_dir
            os.environ.setdefault("HF_HOME", os.path.join(models_dir, "hf"))

        self._preferred_device = preferred_device.lower()
        self._min_free_vram_mb = min_free_vram_mb
        self._pipelines: dict[str, Any] = {}
        self._active_device = "cpu"
        self._paddle_device_arg = "cpu"
        self._fallback_reason: str | None = None
        self._paddle_version = "unknown"
        self._paddleocr_version = "unknown"
        self._gpu_snapshot = inspect_nvml_gpu()

    @property
    def active_device(self) -> str:
        return self._active_device

    @property
    def fallback_reason(self) -> str | None:
        return self._fallback_reason

    def initialize(self) -> dict[str, Any]:
        """Select execution device, run CUDA smoke check, and warm up PP-OCRv5 Mobile."""
        import paddle
        import paddleocr

        self._paddle_version = getattr(paddle, "__version__", "unknown")
        self._paddleocr_version = getattr(paddleocr, "__version__", "unknown")
        self._gpu_snapshot = inspect_nvml_gpu()

        want_cuda = self._preferred_device in ("cuda", "gpu", "auto")
        if want_cuda:
            if not self._gpu_snapshot.available:
                self._fallback_reason = "no_nvidia_gpu_detected"
                self._set_cpu_mode()
            elif self._gpu_snapshot.vram_free_mb < self._min_free_vram_mb:
                self._fallback_reason = (
                    f"insufficient_vram_free_{self._gpu_snapshot.vram_free_mb}mb"
                )
                self._set_cpu_mode()
            elif not paddle.is_compiled_with_cuda():
                self._fallback_reason = "paddle_not_compiled_with_cuda"
                self._set_cpu_mode()
            else:
                try:
                    paddle.set_device("gpu:0")
                    # Real CUDA tensor op + synchronize to verify runtime & cuBLAS/cuDNN
                    a = paddle.randn([64, 64], dtype="float32")
                    b = paddle.randn([64, 64], dtype="float32")
                    c = paddle.matmul(a, b)
                    _ = float(c[0, 0].item())
                    paddle.device.synchronize()
                    del a, b, c
                    self._active_device = "cuda"
                    self._paddle_device_arg = "gpu:0"
                except Exception as exc:
                    self._fallback_reason = f"cuda_init_failed: {exc}"
                    self._set_cpu_mode()
        else:
            self._set_cpu_mode()

        # Load primary PP-OCRv5 Mobile pipeline and execute a warmup inference
        t0 = time.perf_counter()
        try:
            pipeline = self._get_or_create_pipeline("PP-OCRv5_mobile_rec")
            warmup_img = np.full((64, 240, 3), 255, dtype=np.uint8)
            cv2.putText(
                warmup_img,
                "OK",
                (16, 46),
                cv2.FONT_HERSHEY_SIMPLEX,
                1.1,
                (0, 0, 0),
                2,
                cv2.LINE_AA,
            )
            _ = list(pipeline.predict(warmup_img))
        except Exception as exc:
            if self._active_device == "cuda":
                self._fallback_reason = f"paddleocr_cuda_warmup_failed: {exc}"
                self._recover_to_cpu()
                pipeline = self._get_or_create_pipeline("PP-OCRv5_mobile_rec")
                warmup_img = np.full((64, 240, 3), 255, dtype=np.uint8)
                _ = list(pipeline.predict(warmup_img))
            else:
                raise

        init_ms = round((time.perf_counter() - t0) * 1000.0, 2)
        snap = inspect_nvml_gpu()
        self._gpu_snapshot = snap
        return {
            "device": self._active_device,
            "fallback_reason": self._fallback_reason,
            "gpu_available": snap.available,
            "gpu_name": snap.name,
            "driver_version": snap.driver_version,
            "vram_total_mb": snap.vram_total_mb,
            "vram_used_mb": snap.vram_used_mb,
            "vram_free_mb": snap.vram_free_mb,
            "gpu_utilization_pct": snap.gpu_utilization_pct,
            "paddle_version": self._paddle_version,
            "paddleocr_version": self._paddleocr_version,
            "init_latency_ms": init_ms,
        }

    def _set_cpu_mode(self) -> None:
        import paddle

        paddle.set_device("cpu")
        self._active_device = "cpu"
        self._paddle_device_arg = "cpu"

    def _recover_to_cpu(self) -> None:
        import paddle

        self._pipelines.clear()
        gc.collect()
        try:
            if paddle.is_compiled_with_cuda():
                paddle.device.cuda.empty_cache()
        except Exception:
            pass
        self._set_cpu_mode()

    def _get_or_create_pipeline(self, rec_model_name: str) -> Any:
        if rec_model_name in self._pipelines:
            return self._pipelines[rec_model_name]

        # Avoid loading multiple extra models concurrently if VRAM is constrained.
        if self._active_device == "cuda" and len(self._pipelines) >= 1:
            snap = inspect_nvml_gpu()
            if snap.available and snap.vram_free_mb < 1024:
                self._pipelines.clear()
                gc.collect()
                import paddle

                paddle.device.cuda.empty_cache()

        from paddleocr import PaddleOCR

        extra_kwargs: dict[str, Any] = {}
        if self._active_device == "cpu":
            extra_kwargs["enable_mkldnn"] = False

        pipeline = PaddleOCR(
            text_detection_model_name="PP-OCRv5_mobile_det",
            text_recognition_model_name=rec_model_name,
            use_doc_orientation_classify=False,
            use_doc_unwarping=False,
            use_textline_orientation=False,
            device=self._paddle_device_arg,
            **extra_kwargs,
        )
        self._pipelines[rec_model_name] = pipeline
        return pipeline

    @staticmethod
    def decode_image(
        payload: bytes,
        pixel_format: str,
        width: int = 0,
        height: int = 0,
        stride: int = 0,
    ) -> np.ndarray:
        """Decode raw BGRA8/RGBA8/BGR8 pixels or encoded PNG/JPEG bytes into BGR uint8."""
        fmt = pixel_format.lower()
        if fmt == "bgra8":
            if width <= 0 or height <= 0:
                raise ValueError("width and height are required for bgra8 frames")
            row_bytes = stride if stride >= width * 4 else width * 4
            expected = row_bytes * height
            if len(payload) < expected:
                raise ValueError(f"bgra8 buffer too small: {len(payload)} < {expected}")
            arr = np.frombuffer(payload, dtype=np.uint8, count=expected).reshape(
                (height, row_bytes // 4, 4)
            )
            return np.ascontiguousarray(arr[:, :width, :3])
        if fmt == "rgba8":
            if width <= 0 or height <= 0:
                raise ValueError("width and height are required for rgba8 frames")
            row_bytes = stride if stride >= width * 4 else width * 4
            expected = row_bytes * height
            arr = np.frombuffer(payload, dtype=np.uint8, count=expected).reshape(
                (height, row_bytes // 4, 4)
            )
            return np.ascontiguousarray(arr[:, :width, [2, 1, 0]])
        if fmt == "bgr8":
            if width <= 0 or height <= 0:
                raise ValueError("width and height are required for bgr8 frames")
            expected = width * height * 3
            arr = np.frombuffer(payload, dtype=np.uint8, count=expected).reshape(
                (height, width, 3)
            )
            return np.ascontiguousarray(arr)

        # Encoded image bytes (PNG, JPEG, BMP, WEBP)
        buf = np.frombuffer(payload, dtype=np.uint8)
        decoded = cv2.imdecode(buf, cv2.IMREAD_COLOR)
        if decoded is None:
            raise ValueError("Failed to decode image buffer")
        return decoded

    def recognize(
        self,
        img_bgr: np.ndarray,
        region_id: str,
        sequence: int,
        source_language: str | None = None,
    ) -> dict[str, Any]:
        """Run PP-OCRv5 Mobile detection + recognition and return structured subtitle output."""
        rec_model, normalized_lang = map_language_to_rec_model(source_language)
        t0 = time.perf_counter()

        try:
            pipeline = self._get_or_create_pipeline(rec_model)
            raw_results = list(pipeline.predict(img_bgr))
        except RuntimeError as exc:
            msg = str(exc).lower()
            if self._active_device == "cuda" and (
                "out of memory" in msg or "resourceexhausted" in msg or "cuda" in msg
            ):
                self._fallback_reason = f"cuda_oom_recovered: {exc}"
                self._recover_to_cpu()
                pipeline = self._get_or_create_pipeline(rec_model)
                raw_results = list(pipeline.predict(img_bgr))
            else:
                raise

        boxes: list[tuple[float, float, float, float, str, float]] = []
        for item in raw_results:
            data = item["res"] if isinstance(item, dict) and "res" in item else item
            texts = data["rec_texts"] if "rec_texts" in data else []
            scores = data["rec_scores"] if "rec_scores" in data else []
            polys = data["rec_polys"] if "rec_polys" in data else []

            for idx, raw_text in enumerate(texts):
                cleaned = str(raw_text).strip()
                if not cleaned:
                    continue
                score = float(scores[idx]) if idx < len(scores) else 0.0
                if idx < len(polys) and len(polys[idx]) > 0:
                    pts = np.asarray(polys[idx], dtype=np.float32)
                    cy = float(np.mean(pts[:, 1]))
                    min_x = float(np.min(pts[:, 0]))
                    max_x = float(np.max(pts[:, 0]))
                    box_h = max(float(np.max(pts[:, 1]) - np.min(pts[:, 1])), 10.0)
                else:
                    cy = float(idx * 24)
                    min_x = 0.0
                    max_x = float(len(cleaned) * 12)
                    box_h = 20.0
                boxes.append((cy, min_x, max_x, box_h, cleaned, score))

        combined_text, avg_conf = merge_subtitle_polygons(boxes)
        latency_ms = int(round((time.perf_counter() - t0) * 1000.0))

        return {
            "region_id": region_id,
            "sequence": int(sequence),
            "source_language": normalized_lang,
            "text": combined_text,
            "confidence": avg_conf,
            "inference_device": self._active_device,
            "latency_ms": max(latency_ms, 1) if combined_text else latency_ms,
            "fallback_reason": self._fallback_reason,
        }

    def shutdown(self) -> None:
        """Release PaddleOCR model instances and CUDA cache cleanly."""
        self._pipelines.clear()
        gc.collect()
        try:
            import paddle

            if paddle.is_compiled_with_cuda():
                paddle.device.cuda.empty_cache()
        except Exception:
            pass
