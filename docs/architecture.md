# LumaTranslate Architecture

## System Overview

LumaTranslate splits native Windows UI presentation, real-time capture/pipeline orchestration, and local CUDA inference across dedicated processes with strict failure isolation and bounded queues:

```text
LumaTranslate.WinUI.exe (C# / WinUI 3 / Fluent MVVM, Vietnamese UI)
    │  ├── Main Shell (TranslatePage, HistoryPage, SettingsPage)
    │  ├── RegionSelectorWindow (DPI-aware virtual desktop rectangle picker)
    │  └── SubtitleOverlayWindow (Topmost, click-through, WDA_EXCLUDEFROMCAPTURE)
    │
    ▼  Windows Named Pipe (\\.\pipe\LumaTranslate.IPC, length-prefixed UTF-8 JSON)
LumaTranslate.Backend.exe (Rust + Tokio)
    ├── luma-capture      : Windows.Graphics.Capture + D3D11 GPU subresource region crop
    ├── luma-regions      : Multi-monitor clamping, DPI math, 2×2 luma FrameDiffDetector
    ├── luma-core         : Domain contracts, framed binary/JSON codec, OcrStabilizer
    ├── luma-ocr-client   : Supervised child worker manager & bounded request dispatch
    ├── luma-translation  : MyMemory, Azure Translator v3.0, Google Cloud Translation v2
    ├── luma-storage      : Bounded LRU memory cache + SQLite WAL persistence & history
    ├── luma-tts-client   : Supervised VieNeu-TTS v3 Turbo streaming & cancellation
    ├── luma-audio        : Dedicated WASAPI/rodio in-memory f32 PCM playback thread
    └── luma-ipc          : Full-duplex Named Pipe server & event broadcaster
            │
            ├──► workers/ocr/worker.py (Python 3.12 isolated venv)
            │      └── PaddleOCR 3.6.0 (PP-OCRv5 Mobile) + PaddlePaddle GPU 3.3.1 (cu126)
            │
            └──► workers/tts/worker.py (Python 3.12 isolated venv)
                   └── VieNeu-TTS v3 Turbo (vieneu 3.8.5) + PyTorch 2.8.0 (cu128) / ONNX CPU
```

## Real-Time Concurrency & Stale-Result Guarantees

1. **Zero UI-Thread Blocking**: All screen capture, OCR, network HTTP calls, SQLite writes, and audio playback execute inside the Rust Tokio runtime and dedicated native threads. WinUI 3 communicates strictly asynchronously over `NamedPipeClientStream`.
2. **GPU Region Cropping (`CopySubresourceRegion`)**: `luma-capture` keeps `ID3D11Texture2D` frames on the GPU and copies only the `D3D11_BOX` matching each user-selected subtitle rectangle into a small reusable `D3D11_USAGE_STAGING` texture, avoiding full-frame 1080p/1440p/4K readbacks.
3. **Lightweight Change Detection + Subtitle Stabilization**:
   - `FrameDiffDetector` samples a 2×2 luma grid and skips OCR on static frames unless `OcrStabilizer` has an unconfirmed candidate waiting for confirmation.
   - `OcrStabilizer` waits for dialogue to finish appearing (typewriter effect), ignores minor OCR character flicker (`>= 90%` normalized similarity on long lines), and tracks independent sequence numbers per region.
4. **Sequence-ID Stale Protection**: Each region increments a monotonic `sequence` counter. If a slower earlier cloud translation returns after a newer subtitle has already stabilized, `PipelineCoordinator` discards the stale response so older dialogue never overwrites newer subtitles.
5. **Latest-Subtitle-Wins Speech**: When a new Vietnamese translation arrives, `SupervisedTtsClient` immediately stops active WASAPI playback (`AudioPlayer::stop_immediately`), sends a live cancellation signal to `workers/tts/worker.py`, and streams 48 kHz `f32le` PCM chunks directly into memory without writing temporary `.wav` files to disk.

## Subtitle Overlay Capture-Exclusion & Feedback Prevention

1. **Primary OS Exclusion**: `SubtitleOverlayWindow` invokes Win32 `SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE)` (`0x00000011`) so the Desktop Window Manager omits the Vietnamese overlay from `Windows.Graphics.Capture` surfaces while keeping it visible on screen.
2. **Secondary Stabilizer Feedback Guard**: `PipelineCoordinator` registers the currently displayed Vietnamese translation inside `OcrStabilizer`. Even if capture exclusion were unavailable on a restricted display configuration and a capture region overlapped the overlay, `OcrStabilizer` rejects self-captured Vietnamese overlay text.

## Exclusive-Fullscreen & Protected-Content Limitations

- **Exclusive Fullscreen**: Games running in legacy D3D9/D3D11 Exclusive Fullscreen bypass desktop composition, which can prevent `SubtitleOverlayWindow` from appearing above the game surface. Switch the game to **Borderless Windowed** or **Windowed** mode (standard DX12 flip-model games on Windows 11 work natively).
- **DRM & Anti-Cheat**: LumaTranslate uses standard OS `Windows.Graphics.Capture` APIs and never injects DLLs, hooks game processes, or bypasses HDCP/DRM/anti-cheat protections. Protected DRM video surfaces will render black frames by OS design.
