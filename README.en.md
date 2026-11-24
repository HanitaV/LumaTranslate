# LumaTranslate

🌐 **Language / Ngôn ngữ**: [Tiếng Việt](README.md) | **English**

**LumaTranslate** is a native Windows 11 screen-region subtitle translator and local Vietnamese reader inspired by LunaTranslator, built with a **WinUI 3 (C#)** Vietnamese Fluent interface, a **Rust + Tokio** real-time core backend, **CUDA-first PaddleOCR PP-OCRv5 Mobile**, **Cloud Translation to Vietnamese (`vi`)**, and **local VieNeu-TTS v3 Turbo (`48 kHz`)** speech synthesis.

## Key Features

- **Multi-Region & Multi-Monitor Screen Capture**: Drag-to-select one or multiple subtitle regions across DPI-scaled displays, powered by `Windows.Graphics.Capture` + `Direct3D 11` GPU subresource cropping (`CopySubresourceRegion`).
- **CUDA-First Local OCR**: Persistent **PaddleOCR PP-OCRv5 Mobile** worker (`paddlepaddle-gpu 3.3.1 cu126`) supporting Japanese, English, Simplified & Traditional Chinese, Korean, and Latin scripts (`~36–77 ms` on RTX 3060 Laptop GPU) with automatic CPU & OOM fallback.
- **Subtitle Stabilization & Noise Filtering**: Lightweight `FrameDiffDetector` skips static frames; `OcrStabilizer` waits for typewriter dialogue to finish appearing and filters minor OCR character jitter.
- **Vietnamese-Only Cloud Translation**: Supports **MyMemory** (default, no API key required), **Microsoft Azure Translator** (official REST v3.0, F0 free tier compatible), and **Google Cloud Translation** (official REST v2) with explicit user privacy consent, connection testing, in-flight request deduplication, and stale out-of-order protection.
- **Two-Tier Cache & Conversation History**: Bounded in-memory LRU cache backed by **SQLite WAL** (`%LOCALAPPDATA%\LumaTranslate\data\luma_translate.db`) plus searchable Vietnamese conversation history.
- **Topmost Subtitle Overlay**: Borderless, translucent, click-through overlay excluded from Windows Graphics Capture via `SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)` plus secondary feedback filtering.
- **Automatic Local Vietnamese TTS**: Streams `48 kHz` audio from **VieNeu-TTS v3 Turbo** (`~186 ms` first-chunk latency on RTX 3060 CUDA, automatic ONNX CPU fallback when VRAM is constrained) directly into Windows WASAPI with `latest-subtitle-wins` interruption.

## System Requirements

- **OS**: Windows 11 x64 (or Windows 10 version 2004 / build 19041+)
- **GPU**: NVIDIA RTX 3060 (6 GB Laptop or 12 GB Desktop) with Driver `>= 550.54` recommended for CUDA acceleration (graceful CPU fallback included)
- **Pre-installed Dependencies**: **None required** — the app automatically provisions isolated Python runtimes and AI models on first launch.

## Quick Start (Prebuilt Installer)

### 1. Install LumaTranslate

1. Open [GitHub Releases](https://github.com/HanitaV/LumaTranslate/releases), scroll to the bottom of the latest release post, and **click `▸ Assets`** to expand the download list, then download **`LumaTranslate-Setup-*-x64.exe`** *(or grab `*-portable.zip` if you prefer a portable folder)*.
2. Run **`LumaTranslate-Setup-*-x64.exe`**: the per-user installer installs LumaTranslate into `%LOCALAPPDATA%\Programs\LumaTranslate` (**no Administrator / UAC prompt required**), creates **Start Menu** (and optional **Desktop**) shortcuts, and launches LumaTranslate.

### 2. Automatic First-Run AI Setup & Usage

`LumaTranslate.exe` serves as **both the first-run AI environment installer and the application launcher**:

- **First Launch (Unconfigured PC)**: Automatically opens the Vietnamese **Thiết lập môi trường LumaTranslate** wizard, detects NVIDIA GPU/VRAM & driver compatibility, downloads standalone `uv` + isolated Python 3.12 runtimes into `%LOCALAPPDATA%\LumaTranslate\runtimes\`, installs CUDA/CPU OCR & VieNeu-TTS packages, downloads & verifies Apache-2.0 models in `%LOCALAPPDATA%\LumaTranslate\models\`, and transitions straight into the main app.
- **Subsequent Launches**: Launch **LumaTranslate** from Start Menu or Desktop to immediately start `LumaTranslate.Backend.exe`, open the topmost `SubtitleOverlayWindow`, and enter the main translation view.
- **Upgrading**: Simply run a newer `LumaTranslate-Setup-*-x64.exe` over your existing install — downloaded AI models and translation history under `%LOCALAPPDATA%\LumaTranslate` remain intact.

**Optional CLI Flags**:
- `LumaTranslate.exe --setup` — Force-open the built-in AI environment installer/repair wizard.
- `LumaTranslate.exe --auto-install --exit-after-setup` — Run unattended AI environment provisioning and exit when complete.
- `LumaTranslate.exe --uninstall` — Open the built-in Vietnamese uninstaller dialog.

### 3. Uninstall LumaTranslate

You can uninstall the app (`%LOCALAPPDATA%\Programs\LumaTranslate`) and clean up `%LOCALAPPDATA%\LumaTranslate` (`runtimes`, `models`, `tools`, `data`) from any of the following places:

- **Windows Installed Apps**: **Settings → Apps → Installed apps → LumaTranslate → Uninstall** (prompts whether to also remove downloaded multi-GB AI models and translation history; click **Modify** to launch the AI environment repair wizard).
- **In-App Settings**: Open **Cài đặt → Quản lý dữ liệu, môi trường AI & Gỡ cài đặt phần mềm → Gỡ cài đặt phần mềm...** (or click **Gỡ cài đặt / Dọn dẹp...** on the Setup Wizard).
- **PowerShell (unattended / portable)**: Run `powershell -ExecutionPolicy Bypass -File .\uninstall.ps1` in the install folder (add `-Unattended -RemoveAppFolder` for silent removal).

---

## For Developers (Run & Build from Source)

### Required Toolchains

- Rust `1.80+` (`rustc` / `cargo`)
- .NET `10.0` SDK + Windows App SDK `2.7+`
- Python `3.12` x64 + [`uv`](https://github.com/astral-sh/uv)
- [Inno Setup 6](https://jrsoftware.org/isinfo.php) (`winget install JRSoftware.InnoSetup` — optional, used to compile `LumaTranslate-Setup-x64.exe`)

### 1. Setup Inference Environments & Models

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\setup.ps1 -AcceptModelDownload
```

### 2. Verify CUDA OCR & VieNeu-TTS Execution

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\verify-cuda.ps1
```

### 3. Build & Run from Source

```powershell
# Build Rust backend (Release) & WinUI 3 x64 frontend
powershell -ExecutionPolicy Bypass -File .\scripts\build.ps1 -Configuration Release

# Launch the locally built binary
.\apps\LumaTranslate.WinUI\bin\x64\Release\net10.0-windows10.0.26100.0\win-x64\LumaTranslate.exe
```

### 4. Run Automated Tests

```powershell
# Full Rust unit/integration + Python worker + Real CUDA GPU tests
powershell -ExecutionPolicy Bypass -File .\scripts\test.ps1

# Portable tests only (skipping live GPU model tests)
powershell -ExecutionPolicy Bypass -File .\scripts\test.ps1 -SkipGpu
```

### 5. Build `LumaTranslate-Setup-x64.exe` Installer & Distribution

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\package.ps1
# Stages app files in dist\LumaTranslate-win-x64\
# and compiles per-user AppData installer at dist\LumaTranslate-Setup-x64.exe
```

## Documentation

- [Architecture & Concurrency](docs/architecture.md)
- [CUDA Setup & RTX 3060 VRAM Budgeting](docs/cuda-setup.md)
- [Cloud Translation & Privacy](docs/cloud-translation.md)
- [Performance & Measured Benchmarks](docs/performance.md)
- [Troubleshooting](docs/troubleshooting.md)

## License

Code in this repository is licensed under the [MIT License](LICENSE). Downloaded model weights (`PP-OCRv5 Mobile`, `VieNeu-TTS v3 Turbo`, `MOSS-Audio-Tokenizer-Nano`) are licensed under **Apache-2.0** by their respective authors and stored locally under `%LOCALAPPDATA%\LumaTranslate\models\`.
