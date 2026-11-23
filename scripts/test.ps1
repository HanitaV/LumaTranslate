[CmdletBinding()]
param(
    [switch]$SkipGpu
)

$ErrorActionPreference = "Stop"
$RepoRoot = Split-Path -Parent $PSScriptRoot

Write-Host "=== [1/4] Rust Unit & Pipeline Integration Tests ===" -ForegroundColor Cyan
Push-Location (Join-Path $RepoRoot "backend")
try {
    cargo test --workspace
    if ($LASTEXITCODE -ne 0) { throw "Rust workspace tests failed" }
} finally {
    Pop-Location
}

$ocrPy = Join-Path $RepoRoot "workers\ocr\.venv\Scripts\python.exe"
$ttsPy = Join-Path $RepoRoot "workers\tts\.venv\Scripts\python.exe"

Write-Host "=== [2/4] Python Worker Protocol Unit Tests ===" -ForegroundColor Cyan
& $ocrPy -m pytest (Join-Path $RepoRoot "tests\unit") -v
if ($LASTEXITCODE -ne 0) { throw "Python unit tests failed" }

if (-not $SkipGpu) {
    Write-Host "=== [3/4] Real CUDA OCR Inference & OOM Recovery Tests ===" -ForegroundColor Cyan
    & $ocrPy -m pytest (Join-Path $RepoRoot "tests\gpu\test_cuda_ocr.py") -v
    if ($LASTEXITCODE -ne 0) { throw "CUDA OCR tests failed" }

    Write-Host "=== [4/5] Real VieNeu-TTS v3 Turbo CUDA/CPU & Cancellation Tests ===" -ForegroundColor Cyan
    & $ttsPy -m pytest (Join-Path $RepoRoot "tests\gpu\test_cuda_tts.py") -v
    if ($LASTEXITCODE -ne 0) { throw "CUDA TTS tests failed" }

    Write-Host "=== [5/5] Subprocess OCR + TTS Framed IPC End-to-End Integration Test ===" -ForegroundColor Cyan
    & $ocrPy -m pytest (Join-Path $RepoRoot "tests\integration\test_worker_subprocess_e2e.py") -v
    if ($LASTEXITCODE -ne 0) { throw "Worker subprocess integration test failed" }
} else {
    Write-Warning "Skipped real GPU tests (-SkipGpu)."
}

Write-Host "All executed test suites passed!" -ForegroundColor Green
