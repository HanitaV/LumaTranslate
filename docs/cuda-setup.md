# CUDA Setup & RTX 3060 Hardware Guide

## Validated Hardware & Runtime Matrix

LumaTranslate is engineered and verified on **Windows 11 x64** targeting both **NVIDIA RTX 3060 Laptop (6 GB VRAM)** and **RTX 3060 Desktop (12 GB VRAM)**.

| Component | Validated Package / Version | Wheel Index / Runtime |
|---|---|---|
| GPU Driver | NVIDIA Driver `>= 550.54` (Verified on `621.01`) | WDDM 3.x (Windows 11 x64) |
| Python Runtime | CPython `3.12.x` x64 (two isolated virtualenvs) | Managed via `uv` (`workers/ocr/.venv`, `workers/tts/.venv`) |
| OCR Framework | `paddlepaddle-gpu==3.3.1` + `paddleocr==3.6.0` | `https://www.paddlepaddle.org.cn/packages/stable/cu126/` |
| OCR CUDA/cuDNN | `nvidia-cuda-runtime-cu12==12.6.77`, `nvidia-cudnn-cu12==9.5.1.17` | Self-contained pip wheels (no system CUDA Toolkit required) |
| OCR Models | `PP-OCRv5_mobile_det`, `PP-OCRv5_mobile_rec`, `korean_PP-OCRv5_mobile_rec`, `latin_PP-OCRv5_mobile_rec` | Apache-2.0 (`%LOCALAPPDATA%\LumaTranslate\models\paddleocr`) |
| TTS Framework | `vieneu==3.8.5` + `torch==2.8.0+cu128` + `onnxruntime>=1.20.0` | `https://download.pytorch.org/whl/cu128` |
| TTS Models | `pnnbao-ump/VieNeu-TTS-v3-Turbo` + `OpenMOSS-Team/MOSS-Audio-Tokenizer-Nano` | Apache-2.0 (`%LOCALAPPDATA%\LumaTranslate\models\tts`) |

## Why OCR and TTS Use Separate Python Environments

PaddlePaddle GPU (`cu126` + cuDNN 9.5) and PyTorch (`cu128`) ship independent `nvidia-*` CUDA/cuDNN DLL sets in `site-packages`. Installing both into a single Windows Python environment causes DLL resolution collisions (`[WinError 126]` / `[WinError 127]`). Keeping `workers/ocr/.venv` and `workers/tts/.venv` isolated guarantees deterministic CUDA initialization.

## Dynamic VRAM Budgeting (6 GB vs 12 GB RTX 3060)

At startup and during runtime, both AI workers query **NVML** (`nvidia-ml-py` / `pynvml`):

- **OCR (`PaddleOcrEngine`)**: Requires `>= 650 MB` free VRAM to initialize on CUDA (~320 MB active footprint). Performs a real `paddle.matmul` + `PP-OCRv5_mobile` warmup check before reporting `"cuda"`.
- **TTS (`VieNeuTtsEngine`)**: In `Auto` mode, uses PyTorch CUDA (`max_streams=1, max_batch_size=1`, ~930 MB VRAM) when free VRAM is `>= 1400 MB`, and automatically selects **ONNX Runtime CPU** when a game is consuming most of a 6 GB RTX 3060 Laptop's VRAM.
- **OOM Recovery**: If either worker encounters a `CUDA out of memory` runtime exception while a game spikes VRAM usage, it releases CUDA caches (`empty_cache()`), switches cleanly to CPU execution, and completes the request without crashing.

## Automated Setup & Verification Commands

```powershell
# Full environment setup + model download + CUDA smoke verification
powershell -ExecutionPolicy Bypass -File .\scripts\setup.ps1 -AcceptModelDownload

# Standalone real CUDA OCR & VieNeu-TTS verification
powershell -ExecutionPolicy Bypass -File .\scripts\verify-cuda.ps1
```
