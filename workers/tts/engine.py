"""Local VieNeu-TTS v3 Turbo engine with CUDA/CPU selection, streaming, and cancellation."""

from __future__ import annotations

import gc
import inspect
import os
import threading
import time
from typing import Any, Iterator

import numpy as np


def inspect_nvml_vram(device_index: int = 0) -> tuple[bool, str | None, int, int, int]:
    """Return (available, gpu_name, total_mb, used_mb, free_mb) via NVML."""
    try:
        import pynvml

        pynvml.nvmlInit()
        try:
            if pynvml.nvmlDeviceGetCount() <= device_index:
                return False, None, 0, 0, 0
            handle = pynvml.nvmlDeviceGetHandleByIndex(device_index)
            raw_name = pynvml.nvmlDeviceGetName(handle)
            name = raw_name.decode("utf-8") if isinstance(raw_name, bytes) else str(raw_name)
            mem = pynvml.nvmlDeviceGetMemoryInfo(handle)
            return (
                True,
                name,
                int(mem.total // (1024 * 1024)),
                int(mem.used // (1024 * 1024)),
                int(mem.free // (1024 * 1024)),
            )
        finally:
            try:
                pynvml.nvmlShutdown()
            except Exception:
                pass
    except Exception:
        return False, None, 0, 0, 0


class VieNeuTtsEngine:
    """Persistent VieNeu-TTS v3 Turbo wrapper with CUDA/Auto/CPU execution and OOM recovery."""

    DEFAULT_VOICE = "Hải Đăng"
    SAMPLE_RATE = 48000
    MIN_FREE_VRAM_FOR_CUDA_MB = 1400

    def __init__(
        self,
        execution_mode: str = "auto",
        models_dir: str | None = None,
        voice: str = DEFAULT_VOICE,
        min_free_vram_mb: int = MIN_FREE_VRAM_FOR_CUDA_MB,
    ) -> None:
        if models_dir:
            os.makedirs(models_dir, exist_ok=True)
            os.environ["HF_HOME"] = os.path.join(models_dir, "hf")

        self._execution_mode = execution_mode.strip().lower()
        self._voice = voice or self.DEFAULT_VOICE
        self._min_free_vram_mb = min_free_vram_mb
        self._tts: Any = None
        self._active_device = "cpu"
        self._fallback_reason: str | None = None
        self._sample_rate = self.SAMPLE_RATE
        self._vieneu_version = "unknown"
        self._selected_voice = self._voice

    @property
    def active_device(self) -> str:
        return self._active_device

    @property
    def fallback_reason(self) -> str | None:
        return self._fallback_reason

    @property
    def sample_rate(self) -> int:
        return self._sample_rate

    def _instantiate_vieneu(self, target_device: str) -> Any:
        import importlib.metadata
        from vieneu import Vieneu

        try:
            self._vieneu_version = importlib.metadata.version("vieneu")
        except Exception:
            self._vieneu_version = "unknown"

        if target_device == "cpu":
            return Vieneu(
                mode="v3turbo",
                device="cpu",
                backend="onnx",
                precision="fp32",
            )

        return Vieneu(
            mode="v3turbo",
            device="cuda",
            backend="pytorch",
            max_streams=1,
            max_batch_size=1,
        )

    def _resolve_default_voice(self, instance: Any) -> str:
        try:
            voices = instance.list_preset_voices()
            available_ids: list[str] = []
            for entry in voices:
                if isinstance(entry, (tuple, list)) and len(entry) >= 2:
                    available_ids.append(str(entry[1]))
                    if self._voice in (str(entry[0]), str(entry[1])):
                        return str(entry[1])
                elif isinstance(entry, str):
                    available_ids.append(entry)
                    if entry == self._voice:
                        return entry
            if available_ids:
                return available_ids[0]
        except Exception:
            pass
        return self._voice

    def initialize(self) -> dict[str, Any]:
        """Initialize VieNeu-TTS v3 Turbo on CUDA or CPU and run a real synthesis warmup."""
        gpu_ok, gpu_name, vram_total, vram_used, vram_free = inspect_nvml_vram()
        torch_cuda_ok = False
        try:
            import torch

            torch_cuda_ok = bool(torch.cuda.is_available())
        except Exception:
            torch_cuda_ok = False

        mode = self._execution_mode
        target_device = "cpu"
        if mode == "cpu":
            target_device = "cpu"
        elif mode in ("cuda", "gpu", "auto"):
            if not gpu_ok or not torch_cuda_ok:
                target_device = "cpu"
                self._fallback_reason = "cuda_unavailable"
            elif vram_free < self._min_free_vram_mb and mode == "auto":
                target_device = "cpu"
                self._fallback_reason = f"low_vram_headroom_{vram_free}mb"
            elif vram_free < 800:
                target_device = "cpu"
                self._fallback_reason = f"insufficient_vram_{vram_free}mb"
            else:
                target_device = "cuda"

        t0 = time.perf_counter()
        try:
            self._tts = self._instantiate_vieneu(target_device)
            self._active_device = self._detect_actual_device(self._tts, target_device)
            self._selected_voice = self._resolve_default_voice(self._tts)
            self._sample_rate = int(getattr(self._tts, "sample_rate", self.SAMPLE_RATE))

            # Real streaming synthesis warmup to verify execution & pre-capture CUDA graph / ONNX session
            warmup_chunks = list(
                self._tts.infer_stream("Xin chào.", voice=self._selected_voice)
            )
            if not warmup_chunks or sum(len(c) for c in warmup_chunks) == 0:
                raise RuntimeError("Warmup synthesis returned empty waveform")
        except Exception as exc:
            if target_device == "cuda":
                self._fallback_reason = f"cuda_tts_init_failed: {exc}"
                self._cleanup_cuda()
                self._tts = self._instantiate_vieneu("cpu")
                self._active_device = "cpu"
                self._selected_voice = self._resolve_default_voice(self._tts)
                self._sample_rate = int(getattr(self._tts, "sample_rate", self.SAMPLE_RATE))
                _ = list(self._tts.infer_stream("Xin chào.", voice=self._selected_voice))
            else:
                raise

        init_ms = round((time.perf_counter() - t0) * 1000.0, 2)
        _, _, vram_total, vram_used, vram_free = inspect_nvml_vram()
        return {
            "device": self._active_device,
            "execution_mode": self._execution_mode,
            "fallback_reason": self._fallback_reason,
            "voice": self._selected_voice,
            "sample_rate": self._sample_rate,
            "channels": 1,
            "sample_format": "f32le",
            "vieneu_version": self._vieneu_version,
            "gpu_name": gpu_name,
            "vram_total_mb": vram_total,
            "vram_used_mb": vram_used,
            "vram_free_mb": vram_free,
            "init_latency_ms": init_ms,
        }

    @staticmethod
    def _detect_actual_device(instance: Any, requested: str) -> str:
        backend_str = str(getattr(instance, "backend", "")).lower()
        if backend_str == "onnx":
            return "cpu"
        if backend_str == "pytorch":
            return "cuda" if requested == "cuda" else "cpu"
        for attr in ("device", "_device", "backbone_device"):
            val = getattr(instance, attr, None)
            if val is not None:
                s = str(val).lower()
                if "cuda" in s or "gpu" in s:
                    return "cuda"
                if "cpu" in s:
                    return "cpu"
        return requested

    def stream_synthesize(
        self,
        text: str,
        cancel_event: threading.Event | None = None,
    ) -> Iterator[np.ndarray]:
        """Yield mono float32 PCM chunks in [-1.0, 1.0] with Cooperative cancellation & OOM recovery."""
        if self._tts is None:
            raise RuntimeError("TTS engine is not initialized")

        cleaned = text.strip()
        if not cleaned:
            return

        try:
            yield from self._run_stream(cleaned, cancel_event)
        except RuntimeError as exc:
            msg = str(exc).lower()
            if self._active_device == "cuda" and (
                "out of memory" in msg or "cuda" in msg
            ):
                self._fallback_reason = f"cuda_oom_recovered: {exc}"
                self._tts = None
                self._cleanup_cuda()
                self._tts = self._instantiate_vieneu("cpu")
                self._active_device = "cpu"
                yield from self._run_stream(cleaned, cancel_event)
            else:
                raise

    def _run_stream(
        self,
        cleaned: str,
        cancel_event: threading.Event | None,
    ) -> Iterator[np.ndarray]:
        if hasattr(self._tts, "infer_stream"):
            for chunk in self._tts.infer_stream(cleaned, voice=self._selected_voice):
                if cancel_event is not None and cancel_event.is_set():
                    return
                arr = np.asarray(chunk, dtype=np.float32).reshape(-1)
                if arr.size > 0:
                    yield np.clip(arr, -1.0, 1.0)
        else:
            if cancel_event is not None and cancel_event.is_set():
                return
            arr = np.asarray(
                self._tts.infer(text=cleaned, voice=self._selected_voice),
                dtype=np.float32,
            ).reshape(-1)
            if arr.size > 0:
                yield np.clip(arr, -1.0, 1.0)

    def _cleanup_cuda(self) -> None:
        gc.collect()
        try:
            import torch

            if torch.cuda.is_available():
                torch.cuda.synchronize()
                torch.cuda.empty_cache()
        except Exception:
            pass

    def shutdown(self) -> None:
        """Release VieNeu-TTS model weights and CUDA memory cleanly."""
        if self._tts is not None and hasattr(self._tts, "close"):
            try:
                self._tts.close()
            except Exception:
                pass
        self._tts = None
        self._cleanup_cuda()
