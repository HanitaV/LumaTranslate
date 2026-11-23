[CmdletBinding()]
param(
    [switch]$WriteModelManifest
)

$ErrorActionPreference = "Stop"
$RepoRoot = Split-Path -Parent $PSScriptRoot
$ocrPy = Join-Path $RepoRoot "workers\ocr\.venv\Scripts\python.exe"
$ttsPy = Join-Path $RepoRoot "workers\tts\.venv\Scripts\python.exe"
$smokeScript = Join-Path $RepoRoot "tests\gpu\smoke_and_benchmark.py"

if (-not (Test-Path $ocrPy)) {
    throw "OCR Python environment not found at $ocrPy. Run scripts\setup.ps1 first."
}
if (-not (Test-Path $ttsPy)) {
    throw "TTS Python environment not found at $ttsPy. Run scripts\setup.ps1 first."
}

Write-Host "=== Verifying PaddleOCR PP-OCRv5 Mobile on CUDA ===" -ForegroundColor Cyan
$ocrJsonRaw = & $ocrPy $smokeScript ocr
if ($LASTEXITCODE -ne 0) {
    throw "OCR verification script exited with code $LASTEXITCODE"
}
Write-Host $ocrJsonRaw

Write-Host "=== Verifying VieNeu-TTS v3 Turbo on CUDA & CPU ONNX ===" -ForegroundColor Cyan
$ttsJsonRaw = & $ttsPy $smokeScript tts
if ($LASTEXITCODE -ne 0) {
    throw "TTS verification script exited with code $LASTEXITCODE"
}
Write-Host $ttsJsonRaw

if ($WriteModelManifest) {
    $modelsRoot = Join-Path $env:LOCALAPPDATA "LumaTranslate\models"
    New-Item -ItemType Directory -Force -Path $modelsRoot | Out-Null

    $filesSummary = Get-ChildItem -Path $modelsRoot -Recurse -File -ErrorAction SilentlyContinue |
        Where-Object { $_.Extension -in @(".pdiparams", ".safetensors", ".onnx", ".json") -and $_.Length -gt 1024 } |
        Select-Object -First 25 |
        ForEach-Object {
            [PSCustomObject]@{
                relative_path = $_.FullName.Substring($modelsRoot.Length).TrimStart('\')
                size_bytes    = $_.Length
                sha256        = (Get-FileHash -Algorithm SHA256 -Path $_.FullName).Hash.ToLowerInvariant()
            }
        }

    $manifest = [ordered]@{
        generated_at_utc = (Get-Date).ToUniversalTime().ToString("o")
        models_directory = $modelsRoot
        assets = @(
            [ordered]@{
                name              = "PaddleOCR PP-OCRv5 Mobile (det + rec + korean_rec)"
                version           = "PP-OCRv5 (PaddleOCR 3.6.0 / PaddlePaddle GPU 3.3.1 cu126)"
                source_url        = "https://huggingface.co/PaddlePaddle/PP-OCRv5_mobile_rec"
                license           = "Apache-2.0"
                execution_backend = "PaddlePaddle CUDA 12.6 (CPU fallback supported)"
            },
            [ordered]@{
                name              = "VieNeu-TTS v3 Turbo + MOSS-Audio-Tokenizer-Nano"
                version           = "v3-Turbo (vieneu 3.8.5 / PyTorch 2.8.0+cu128 & ONNX Runtime)"
                source_url        = "https://huggingface.co/pnnbao-ump/VieNeu-TTS-v3-Turbo"
                license           = "Apache-2.0"
                execution_backend = "PyTorch CUDA 12.8 / ONNX Runtime CPU"
            }
        )
        verified_files = $filesSummary
    }

    $manifestPath = Join-Path $modelsRoot "model-manifest.json"
    $manifest | ConvertTo-Json -Depth 6 | Set-Content -Path $manifestPath -Encoding UTF8
    Write-Host "Wrote verified model manifest to $manifestPath" -ForegroundColor Green
}
