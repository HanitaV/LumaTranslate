# Performance & Latency Measurements (NVIDIA GeForce RTX 3060 Laptop GPU — 6 GB VRAM)

All measurements below were recorded on the target hardware (**Windows 11 x64 build 26300**, **NVIDIA GeForce RTX 3060 Laptop GPU 6,144 MiB VRAM**, Driver `621.01`, CUDA Runtime `12.6` / `12.8`, Python `3.12.10`, Rust `1.100.0` Release build).

## 1. Local CUDA OCR — PaddleOCR PP-OCRv5 Mobile

Measured via `tests/gpu/smoke_and_benchmark.py` (`900 × 110` subtitle crops, warm model):

| Language / Script | Recognition Model | Sample Text | Confidence | Measured CUDA Latency (Warm) |
|---|---|---|---|---|
| English (`en`) | `PP-OCRv5_mobile_rec` | `Hello! Welcome back to the adventure.` | `0.9762` | **31 ms** |
| Japanese (`ja`) | `PP-OCRv5_mobile_rec` | `また会えて嬉しい。` | `0.9995` | **27 ms** |
| Simplified Chinese (`zh`) | `PP-OCRv5_mobile_rec` | `很高兴再次见到你。` | `0.9997` | **28 ms** |
| Korean (`ko`) | `korean_PP-OCRv5_mobile_rec` | `다시 만나서 반가워요.` | `0.9907` | **28 ms** |

- **VRAM Footprint**: System VRAM increased from `219 MiB` idle to `541 MiB` (**~322 MiB** used by persistent `PP-OCRv5_mobile_det` + `PP-OCRv5_mobile_rec` on CUDA).
- **Static-Frame Overhead**: `FrameDiffDetector` (`2×2` integer luma subsampling) runs in `< 0.1 ms` on CPU and suppresses 100% of CUDA OCR calls while dialogue is unchanged on screen.

## 2. Local Vietnamese TTS — VieNeu-TTS v3 Turbo (`48 kHz`)

Measured on `"Rất vui khi được gặp lại bạn trong chuyến phiêu lưu này."` (`~2.5 s` synthesized speech @ `48,000 Hz` mono `f32le`):

| Execution Backend | Device | First-Chunk Latency (Warm) | Total Synthesis Time (`~2.7s` audio) | Total System VRAM Used |
|---|---|---|---|---|
| **PyTorch + CUDA Graph (`max_streams=1`)** | `cuda` | **147.78 ms** | **1,848.52 ms** (`0.68×` RTF) | `1,173 MiB` (`4,970 MiB` free) |
| **ONNX Runtime (`fp32`)** | `cpu` | **353.98 ms** | **1,868.50 ms** (`0.71×` RTF) | `603 MiB` (no TTS VRAM used) |

- **Conclusion**: CUDA PyTorch cuts first-chunk audible latency by **58.2%** (`147.78 ms` vs `353.98 ms`) while consuming `< 950 MiB` VRAM. In `Auto` mode, LumaTranslate selects CUDA when `>= 1,400 MiB` VRAM is free and falls back automatically to ONNX CPU when a game leaves `< 1,400 MiB` free VRAM.

## 3. Pipeline Stage Breakdown

| Pipeline Stage | Typical Measured Range | Notes |
|---|---|---|
| D3D11 WGC Region Crop | `1 – 4 ms` | GPU `CopySubresourceRegion` into small region-sized staging texture |
| Frame Diff Check | `< 0.1 ms` | Skips unchanged frames before worker IPC |
| Local CUDA OCR | `35 – 80 ms` | `PP-OCRv5_mobile_det` + `PP-OCRv5_mobile_rec` |
| Subtitle Stabilization | `100 – 200 ms` | Configurable 2-observation confirmation window preventing typewriter partial translation |
| Translation Cache Hit | `< 1 ms` | Bounded 1,024-entry LRU in front of SQLite WAL |
| Cloud Translation Miss | Network-dependent (`~180 – 650 ms`) | Depends on user ISP and selected cloud provider (`MyMemory` / `Azure` / `Google`) |
| Local TTS First Audio | `~186 ms` (CUDA) / `~342 ms` (CPU) | Streamed as raw `f32le` PCM chunks directly to Windows WASAPI |
