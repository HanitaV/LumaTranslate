# Troubleshooting

## 1. `LumaTranslate.Backend.exe` Not Found or IPC Disconnected
- Run `powershell -ExecutionPolicy Bypass -File .\scripts\build.ps1` to compile both the Rust backend and WinUI 3 frontend and place `LumaTranslate.Backend.exe` alongside `LumaTranslate.WinUI.exe`.
- Verify no stale process is holding a custom pipe; by default LumaTranslate uses `\\.\pipe\LumaTranslate.IPC`.

## 2. OCR or TTS Runs on CPU Instead of CUDA
- Run `nvidia-smi` to confirm your NVIDIA driver is `>= 550.54` (tested with `621.01`).
- Run `powershell -ExecutionPolicy Bypass -File .\scripts\verify-cuda.ps1` to inspect NVML free VRAM and worker initialization output.
- On a **6 GB RTX 3060 Laptop**, if an active 3D game leaves `< 1,400 MB` free VRAM, `VieNeu-TTS` in **Tự động (`Auto`)** mode intentionally selects **ONNX Runtime CPU** to avoid starving the game of VRAM while keeping **PaddleOCR PP-OCRv5 Mobile** on CUDA (`>= 650 MB` threshold).

## 3. Subtitle Overlay Not Visible Above Fullscreen Game
- Switch the game's display mode from **Exclusive Fullscreen** to **Borderless Windowed** (`Không viền`) or **Windowed**. Exclusive fullscreen D3D modes bypass the Windows Desktop Window Manager where topmost translucent overlays render.

## 4. Repositioning the Subtitle Overlay
- By default, **Khóa xuyên chuột (Click-through)** is enabled so mouse clicks pass straight through the subtitle into your game.
- Open **Cài đặt → Cửa sổ phụ đề nổi**, turn **Khóa xuyên chuột** off, drag the subtitle overlay to your preferred monitor/position, then turn **Khóa xuyên chuột** back on and click **Lưu tất cả cài đặt**.

## 5. Cloud Translation Quota or Connection Errors
- **MyMemory HTTP 429 / Quota**: Enter an email address in **Cài đặt** for a higher daily free allowance, or configure an **Azure Translator (F0 Free)** or **Google Cloud Translation** API key.
- **Consent Warning**: Online translation will not start until you click **Tôi đồng ý bật dịch trực tuyến** on the main page or check the privacy consent box in **Cài đặt**.
