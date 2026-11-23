"""Supervised local VieNeu-TTS worker supporting streaming audio chunks and live cancellation.

Runs 100% single-threaded so PyTorch CUDA graphs, ONNX Runtime sessions, and MSVCRT stdio
locks never contend across threads, while polling `PeekNamedPipe` on stdin between audio
chunks to abort obsolete utterances immediately when a `cancel` or newer `synthesize` arrives.
"""

from __future__ import annotations

import ctypes
import os
import sys
import time
import traceback
from pathlib import Path

import numpy as np

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
from tts.engine import VieNeuTtsEngine, inspect_nvml_vram  # noqa: E402


def stdin_has_pending_data() -> bool:
    """Non-blocking check whether parent process wrote a new frame (e.g. cancel/supersede) to stdin."""
    if os.name != "nt":
        return False
    try:
        import msvcrt

        handle = msvcrt.get_osfhandle(sys.stdin.buffer.fileno())
        avail = ctypes.c_ulong(0)
        ok = ctypes.windll.kernel32.PeekNamedPipe(
            ctypes.c_void_p(handle),
            None,
            0,
            None,
            ctypes.byref(avail),
            None,
        )
        return bool(ok) and avail.value > 0
    except Exception:
        return False


def main() -> int:
    ipc_out = reserve_clean_ipc_stdout()
    writer = FramedWriter(ipc_out)
    engine: VieNeuTtsEngine | None = None
    log_stderr("tts-worker", f"Started PID={os.getpid()}")

    try:
        while True:
            framed = read_frame()
            if framed is None:
                log_stderr("tts-worker", "Clean stdin EOF received; shutting down")
                break

            header, _ = framed
            cmd = str(header.get("cmd", "")).lower()
            req_id = header.get("request_id")

            if cmd == "init":
                try:
                    if engine is not None:
                        engine.shutdown()
                        engine = None
                    engine = VieNeuTtsEngine(
                        execution_mode=str(header.get("execution_mode", "auto")),
                        models_dir=header.get("models_dir"),
                        voice=str(header.get("voice", VieNeuTtsEngine.DEFAULT_VOICE)),
                        min_free_vram_mb=int(header.get("min_free_vram_mb", 1400)),
                    )
                    info = engine.initialize()
                    log_stderr(
                        "tts-worker",
                        f"Initialized VieNeu-TTS on device={info['device']} voice={info['voice']} "
                        f"sr={info['sample_rate']} fallback={info['fallback_reason']}",
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
                    log_stderr("tts-worker", f"Init failed: {traceback.format_exc()}")
                    writer.send(
                        {
                            "ok": False,
                            "request_id": req_id,
                            "error": str(exc),
                        }
                    )

            elif cmd == "synthesize":
                utt_id = int(header.get("utterance_id", 0))
                text = str(header.get("text", ""))
                if engine is None:
                    writer.send(
                        {
                            "ok": False,
                            "request_id": req_id,
                            "utterance_id": utt_id,
                            "error": "TTS engine is not initialized",
                        }
                    )
                    continue

                # If another frame is ALREADY waiting on stdin behind this synthesize request,
                # skip obsolete synthesis immediately (`latest-subtitle-wins`).
                if stdin_has_pending_data():
                    writer.send(
                        {
                            "ok": True,
                            "request_id": req_id,
                            "utterance_id": utt_id,
                            "event": "cancelled",
                            "latency_ms": 0,
                        }
                    )
                    continue

                t0 = time.perf_counter()
                first_chunk_ms: int | None = None
                total_samples = 0
                chunk_idx = 0
                aborted = False

                try:
                    for pcm_f32 in engine.stream_synthesize(text):
                        if chunk_idx > 0 and stdin_has_pending_data():
                            aborted = True
                            break
                        if first_chunk_ms is None:
                            first_chunk_ms = int(round((time.perf_counter() - t0) * 1000.0))
                        raw_bytes = np.ascontiguousarray(pcm_f32, dtype="<f4").tobytes()
                        total_samples += int(pcm_f32.size)
                        writer.send(
                            {
                                "ok": True,
                                "request_id": req_id,
                                "utterance_id": utt_id,
                                "event": "audio_chunk",
                                "chunk_index": chunk_idx,
                                "sample_rate": engine.sample_rate,
                                "channels": 1,
                                "sample_format": "f32le",
                                "first_chunk_latency_ms": first_chunk_ms,
                                "inference_device": engine.active_device,
                            },
                            raw_bytes,
                        )
                        chunk_idx += 1

                    total_ms = int(round((time.perf_counter() - t0) * 1000.0))
                    if aborted:
                        writer.send(
                            {
                                "ok": True,
                                "request_id": req_id,
                                "utterance_id": utt_id,
                                "event": "cancelled",
                                "latency_ms": total_ms,
                            }
                        )
                    else:
                        writer.send(
                            {
                                "ok": True,
                                "request_id": req_id,
                                "utterance_id": utt_id,
                                "event": "done",
                                "total_samples": total_samples,
                                "sample_rate": engine.sample_rate,
                                "first_chunk_latency_ms": first_chunk_ms or total_ms,
                                "latency_ms": total_ms,
                                "inference_device": engine.active_device,
                                "fallback_reason": engine.fallback_reason,
                            }
                        )
                except Exception as exc:
                    log_stderr("tts-worker", f"Synthesis error: {traceback.format_exc()}")
                    writer.send(
                        {
                            "ok": False,
                            "request_id": req_id,
                            "utterance_id": utt_id,
                            "error": str(exc),
                        }
                    )

            elif cmd == "cancel":
                writer.send(
                    {
                        "ok": True,
                        "request_id": req_id,
                        "event": "cancel_ack",
                        "utterance_id": header.get("utterance_id"),
                    }
                )

            elif cmd in ("ping", "status"):
                _, _, total_mb, used_mb, free_mb = inspect_nvml_vram()
                writer.send(
                    {
                        "ok": True,
                        "request_id": req_id,
                        "device": engine.active_device if engine else "uninitialized",
                        "fallback_reason": engine.fallback_reason if engine else None,
                        "sample_rate": engine.sample_rate if engine else 48000,
                        "vram_total_mb": total_mb,
                        "vram_used_mb": used_mb,
                        "vram_free_mb": free_mb,
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
        log_stderr("tts-worker", f"Protocol error: {exc}")
        return 2
    except KeyboardInterrupt:
        pass
    finally:
        if engine is not None:
            engine.shutdown()
        log_stderr("tts-worker", "Shutdown complete")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
