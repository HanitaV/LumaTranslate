# Repository Guidelines

## Project Structure & Module Organization

LumaTranslate is organized as a multi-process Windows 11 desktop application:

- `apps/LumaTranslate.WinUI/` — Native C# / WinUI 3 (.NET 10, Windows App SDK 2.7, unpackaged x64) Vietnamese UI frontend (`Views/`, `ViewModels/`, `Controls/`, `Overlays/`, `Services/`, `Assets/`).
- `backend/` — Rust Cargo workspace (`LumaTranslate.Backend.exe`) orchestrating real-time capture, scheduling, translation, caching, IPC, and audio:
  - `crates/core` — Domain types, framed binary/JSON worker codec, Named Pipe wire envelopes, and `OcrStabilizer`.
  - `crates/regions` — Multi-monitor clamping, DPI coordinate conversion, and `FrameDiffDetector`.
  - `crates/capture` — `Windows.Graphics.Capture` + `Direct3D 11` GPU region cropping (`CopySubresourceRegion`).
  - `crates/ocr-client` — Supervised PaddleOCR child-process client.
  - `crates/translation` — `TranslationProvider` trait + `MyMemory`, `Azure Translator`, and `Google Cloud Translation` targeting Vietnamese (`vi`).
  - `crates/tts-client` — Supervised `VieNeu-TTS v3 Turbo` streaming client with `latest-subtitle-wins` cancellation.
  - `crates/audio` — Dedicated Windows WASAPI (`rodio`) in-memory `f32` PCM playback thread.
  - `crates/storage` — Bounded in-memory LRU cache + SQLite WAL persistence (`%LOCALAPPDATA%\LumaTranslate\data\`).
  - `crates/ipc` — Full-duplex Windows Named Pipe server (`\\.\pipe\LumaTranslate.IPC`).
  - `crates/app` — Pipeline coordinator and `LumaTranslate.Backend` binary entrypoint.
- `workers/` — Isolated Python 3.12 local inference workers (`workers/ocr/` for PaddleOCR 3.6 + PaddlePaddle GPU 3.3.1 `cu126`; `workers/tts/` for VieNeu-TTS v3 Turbo 3.8.5 + PyTorch `2.8.0+cu128` / ONNX Runtime CPU; `workers/shared/protocol.py` for binary/JSON stdio framing).
- `shared/protocol/` — IPC protocol schema documentation.
- `tests/` — Python unit (`tests/unit/`), worker subprocess integration (`tests/integration/`), multilingual PNG fixtures (`tests/fixtures/`), and real CUDA GPU tests (`tests/gpu/`).
- `scripts/` — Reproducible PowerShell workflows (`setup.ps1`, `verify-cuda.ps1`, `build.ps1`, `test.ps1`, `package.ps1`).
- `installer/` — Runtime provisioning script (`install.ps1`) for packaged distributions.
- `docs/` — Architecture, CUDA setup, cloud translation, performance, and troubleshooting guides.

## Build, Test, and Development Commands

- `powershell -ExecutionPolicy Bypass -File .\scripts\setup.ps1 -AcceptModelDownload` — Verify toolchains, create isolated OCR/TTS Python 3.12 virtualenvs, install pinned CUDA packages, download models, and verify GPU execution.
- `powershell -ExecutionPolicy Bypass -File .\scripts\build.ps1 -Configuration Release` — Build the Rust backend (`cargo build --release`) and WinUI 3 x64 app (`dotnet build`).
- `powershell -ExecutionPolicy Bypass -File .\scripts\test.ps1` — Execute Rust workspace unit/integration tests, Python protocol unit tests, and live CUDA OCR/TTS tests (`-SkipGpu` skips hardware-dependent tests).
- `powershell -ExecutionPolicy Bypass -File .\scripts\package.ps1` — Publish a self-contained Windows x64 distribution under `dist\LumaTranslate-win-x64`.

## Coding Style & Naming Conventions

- **Rust**: Edition 2021, `rustfmt` standard 4-space indentation, `thiserror`/`anyhow` error types, zero warnings (`cargo test --workspace`), documented `// SAFETY:` invariants on all Win32/D3D11 `unsafe` blocks.
- **C# / WinUI 3**: `<Nullable>enable</Nullable>`, `<ImplicitUsings>enable</ImplicitUsings>`, Fluent theme-aware brushes, MVVM separation, 100% Vietnamese user-facing strings, zero blocking `.Result`/`.Wait()` calls on the UI thread.
- **Python Workers**: Python 3.12 type annotations, diagnostics exclusively on `stderr` so `stdout` remains a clean binary-framed IPC stream.

## Testing Guidelines

Add unit and integration tests alongside new behavior. Mock cloud translation providers in automated CI tests rather than depending on public rate-limited endpoints. Any CUDA test in `tests/gpu/` must run real tensor/model inference rather than checking `nvidia-smi` alone.

## Commit & Pull Request Guidelines

Use concise, imperative commit subjects (e.g., `Add Azure Translator fallback handling`) and keep each commit focused on one logical change. Pull requests should describe validation performed and include screenshots for visible WinUI changes.

## Security & Configuration

Never commit credentials, API keys, local SQLite databases, or downloaded model weights. User credentials and runtime data live under `%LOCALAPPDATA%\LumaTranslate\`.

## Subagent & Model Delegation

Whenever a task or subtask can be handled by a lighter model, prioritize delegating it to subagents running **Gemini 4 Hydrogen** (`flash_lite`) or **Gemini 4 Neon** (`flash`) rather than inheriting a heavier parent model:

- **Gemini 4 Hydrogen (`flash_lite`)**: Default choice for trivial, mechanical, high-volume, or well-scoped subtasks (e.g., file/symbol lookups, boilerplate generation, formatting checks, log extraction, or parallel fan-out scans).
- **Gemini 4 Neon (`flash`)**: Preferred choice for straightforward implementation, routine bug fixes, unit test authoring, documentation updates, and targeted codebase or web research.
- **Heavier / Parent Models (`inherit` / `pro`)**: Reserve for complex architecture, cross-cutting refactors, subtle debugging, ambiguity resolution, and top-level orchestration.

Apply this preference consistently across `invoke_subagent` (`Model: "flash_lite"` or `"flash"`), `run_workflow` (`model="flash_lite"` or `"flash"`), and `/swarm` workers (pinning `Gemini 4 Hydrogen` or `Gemini 4 Neon`).

## Codex Skills Compatibility

Antigravity is allowed to discover and use Codex skills alongside native `.agents/skills/`:

- **Discovery Config**: `.agents/skills.json` registers `~/.codex/skills` and `~/.codex/skills/.system` so Antigravity can load Codex skills natively.
- **Usage**: When a task matches a Codex skill (including project-local `.codex/skills/` if added later, or user-level `~/.codex/skills/`), read its `SKILL.md` and follow its instructions, scripts, and references just like a standard Antigravity skill.
