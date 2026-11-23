"""Supervised local OCR worker communicating with the Rust backend over framed stdio."""

from __future__ import annotations

import os
import sys
import traceback
from pathlib import Path

# Allow importing shared worker framing module without global PYTHONPATH setup.
WORKERS_ROOT = Path(__file__).resolve().parents[1]
if str(WORKERS_ROOT) not in sys.path:
    sys.path.insert(0, str(WORKERS_ROOT))

from shared.protocol import (  # noqa: E402
    FramedWriter,
    ProtocolError,
    log_stderr,
    read_frame,
    reserve_clean_ipc_stdout,
)
from ocr.engine import PaddleOcrEngine, inspect_nvml_gpu  # noqa: E402


def main() -> int:
    ipc_out = reserve_clean_ipc_stdout()
    writer = FramedWriter(ipc_out)
    engine: PaddleOcrEngine | None = None
    log_stderr("ocr-worker", f"Started PID={os.getpid()}")

    try:
        while True:
            framed = read_frame()
            if framed is None:
                log_stderr("ocr-worker", "Clean stdin EOF received; shutting down")
                break

            header, payload = framed
            cmd = str(header.get("cmd", "")).lower()
            req_id = header.get("request_id")

            if cmd == "init":
                try:
                    preferred = str(header.get("preferred_device", "cuda"))
                    models_dir = header.get("models_dir")
                    min_vram = int(header.get("min_free_vram_mb", 650))
                    if engine is not None:
                        engine.shutdown()
                    engine = PaddleOcrEngine(
                        preferred_device=preferred,
                        models_dir=models_dir,
                        min_free_vram_mb=min_vram,
                    )
                    info = engine.initialize()
                    log_stderr(
                        "ocr-worker",
                        f"Initialized on device={info['device']} gpu={info['gpu_name']} "
                        f"vram_free={info['vram_free_mb']}MB fallback={info['fallback_reason']}",
                    )
                    writer.send(
                        {
                            "ok": True,
                            "request_id": req_id,
                            "event": "ready",
                            **info,
                        }
                    )
                except Exception as exc:
                    log_stderr("ocr-worker", f"Init failed: {traceback.format_exc()}")
                    writer.send(
                        {
                            "ok": False,
                            "request_id": req_id,
                            "error": str(exc),
                        }
                    )

            elif cmd == "ocr":
                if engine is None:
                    writer.send(
                        {
                            "ok": False,
                            "request_id": req_id,
                            "error": "OCR engine is not initialized",
                        }
                    )
                    continue
                try:
                    img_bgr = engine.decode_image(
                        payload=payload,
                        pixel_format=str(header.get("pixel_format", "bgra8")),
                        width=int(header.get("width", 0)),
                        height=int(header.get("height", 0)),
                        stride=int(header.get("stride", 0)),
                    )
                    result = engine.recognize(
                        img_bgr=img_bgr,
                        region_id=str(header.get("region_id", "region-1")),
                        sequence=int(header.get("sequence", 0)),
                        source_language=header.get("source_language"),
                    )
                    writer.send(
                        {
                            "ok": True,
                            "request_id": req_id,
                            **result,
                        }
                    )
                except Exception as exc:
                    log_stderr("ocr-worker", f"OCR error: {traceback.format_exc()}")
                    writer.send(
                        {
                            "ok": False,
                            "request_id": req_id,
                            "region_id": header.get("region_id", ""),
                            "sequence": header.get("sequence", 0),
                            "error": str(exc),
                        }
                    )

            elif cmd == "ping" or cmd == "status":
                snap = inspect_nvml_gpu()
                writer.send(
                    {
                        "ok": True,
                        "request_id": req_id,
                        "device": engine.active_device if engine else "uninitialized",
                        "fallback_reason": engine.fallback_reason if engine else None,
                        "vram_total_mb": snap.vram_total_mb,
                        "vram_used_mb": snap.vram_used_mb,
                        "vram_free_mb": snap.vram_free_mb,
                        "gpu_utilization_pct": snap.gpu_utilization_pct,
                    }
                )

            elif cmd == "shutdown":
                if engine is not None:
                    engine.shutdown()
                    engine = None
                writer.send({"ok": True, "request_id": req_id, "event": "shutdown"})
                break

            else:
                writer.send(
                    {
                        "ok": False,
                        "request_id": req_id,
                        "error": f"Unknown command: {cmd}",
                    }
                )

    except ProtocolError as exc:
        log_stderr("ocr-worker", f"Protocol error: {exc}")
        return 2
    except KeyboardInterrupt:
        pass
    finally:
        if engine is not None:
            engine.shutdown()
        log_stderr("ocr-worker", "Shutdown complete")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
