[CmdletBinding()]
param(
    [switch]$AcceptModelDownload,
    [switch]$SkipModelDownload
)

$ErrorActionPreference = "Stop"
$RepoRoot = Split-Path -Parent $PSScriptRoot

Write-Host "=== LumaTranslate Environment Setup ===" -ForegroundColor Cyan

# 1. Check Windows version
$osVer = [System.Environment]::OSVersion.Version
Write-Host "[1/10] Windows Version: $osVer"
if ($osVer.Major -lt 10 -or $osVer.Build -lt 19041) {
    throw "Windows 10 build 19041+ or Windows 11 x64 is required (detected $osVer)."
}

# 2 & 3. Check NVIDIA GPU and driver compatibility
Write-Host "[2/10] Inspecting NVIDIA GPU & Driver..."
$nvidiaSmi = Get-Command "nvidia-smi.exe" -ErrorAction SilentlyContinue
$hasCudaGpu = $false
if ($null -ne $nvidiaSmi) {
    $gpuCsv = & nvidia-smi --query-gpu=name,memory.total,memory.free,driver_version --format=csv,noheader,nounits 2>$null
    if ($LASTEXITCODE -eq 0 -and $gpuCsv) {
        $parts = ($gpuCsv | Select-Object -First 1).Split(",") | ForEach-Object { $_.Trim() }
        $gpuName = $parts[0]
        $vramTotal = [int]$parts[1]
        $vramFree = [int]$parts[2]
        $driverVer = $parts[3]
        Write-Host "[3/10] Detected GPU: $gpuName | VRAM: $vramFree/$vramTotal MB free | Driver: $driverVer" -ForegroundColor Green
        $drvMajor = [int]($driverVer.Split(".")[0])
        if ($drvMajor -lt 550) {
            Write-Warning "NVIDIA driver $driverVer is older than 550.x; CUDA 12.6/12.8 wheels may fall back to CPU."
        } else {
            $hasCudaGpu = $true
        }
    }
}
if (-not $hasCudaGpu) {
    Write-Warning "Compatible NVIDIA CUDA GPU/driver not confirmed via nvidia-smi; CPU fallback will remain available."
}

# 4. Check Rust toolchain
Write-Host "[4/10] Checking Rust toolchain..."
$rustc = Get-Command "rustc" -ErrorAction Stop
$cargo = Get-Command "cargo" -ErrorAction Stop
Write-Host "       $(& $rustc --version) | $(& $cargo --version)"

# 5. Check .NET SDK & Windows App SDK templates
Write-Host "[5/10] Checking .NET SDK..."
$dotnet = Get-Command "dotnet" -ErrorAction Stop
$sdks = & $dotnet --list-sdks
if (-not $sdks) {
    throw "No .NET SDK found. Install .NET 10 SDK."
}
Write-Host "       Installed .NET SDKs: $($sdks -join '; ')"

# 6. Locate Python 3.11 or 3.12 for isolated AI worker environments
Write-Host "[6/10] Preparing isolated Python 3.12/3.11 worker environments..."
$pythonExe = $null
foreach ($ver in @("3.12", "3.11")) {
    try {
        $candidate = (& py "-$ver" -c "import sys; print(sys.executable)" 2>$null)
        if ($LASTEXITCODE -eq 0 -and $candidate -and (Test-Path $candidate.Trim())) {
            $pythonExe = $candidate.Trim()
            break
        }
    } catch {}
}
if (-not $pythonExe) {
    $pythonCmd = Get-Command "python" -ErrorAction Stop
    $pythonExe = $pythonCmd.Source
}
Write-Host "       Base Python interpreter: $pythonExe"

$ocrVenv = Join-Path $RepoRoot "workers\ocr\.venv"
$ttsVenv = Join-Path $RepoRoot "workers\tts\.venv"
$uvCmd = Get-Command "uv" -ErrorAction SilentlyContinue

if ($null -ne $uvCmd) {
    if (-not (Test-Path "$ocrVenv\Scripts\python.exe")) {
        & uv venv $ocrVenv --python $pythonExe
    }
    if (-not (Test-Path "$ttsVenv\Scripts\python.exe")) {
        & uv venv $ttsVenv --python $pythonExe
    }

    # 7. Install compatible OCR dependencies
    Write-Host "[7/10] Installing pinned PaddleOCR PP-OCRv5 + CUDA 12.6 dependencies..."
    & uv pip install --python "$ocrVenv\Scripts\python.exe" `
        "paddlepaddle-gpu==3.3.1" `
        --index-url "https://www.paddlepaddle.org.cn/packages/stable/cu126/" `
        --extra-index-url "https://pypi.org/simple" `
        --index-strategy unsafe-best-match
    & uv pip install --python "$ocrVenv\Scripts\python.exe" `
        "paddleocr==3.6.0" "paddlex==3.6.1" "opencv-contrib-python==4.10.0.84" `
        "numpy==2.3.5" "pillow==13.0.0" "nvidia-ml-py==13.615.71" "pytest==9.2.0"

    # 8. Install compatible TTS dependencies
    Write-Host "[8/10] Installing pinned VieNeu-TTS v3 Turbo + PyTorch CUDA 12.8 dependencies..."
    & uv pip install --python "$ttsVenv\Scripts\python.exe" `
        "torch==2.8.0" "torchaudio==2.8.0" `
        --index-url "https://download.pytorch.org/whl/cu128" `
        --extra-index-url "https://pypi.org/simple" `
        --index-strategy unsafe-best-match
    & uv pip install --python "$ttsVenv\Scripts\python.exe" `
        "transformers==4.57.6" "vieneu==3.8.5" "onnxruntime>=1.20.0" `
        "nvidia-ml-py>=12.560.30" "pytest>=8.0.0"
} else {
    if (-not (Test-Path "$ocrVenv\Scripts\python.exe")) { & $pythonExe -m venv $ocrVenv }
    if (-not (Test-Path "$ttsVenv\Scripts\python.exe")) { & $pythonExe -m venv $ttsVenv }
    & "$ocrVenv\Scripts\python.exe" -m pip install --upgrade pip
    & "$ocrVenv\Scripts\python.exe" -m pip install "paddlepaddle-gpu==3.3.1" -i "https://www.paddlepaddle.org.cn/packages/stable/cu126/"
    & "$ocrVenv\Scripts\python.exe" -m pip install "paddleocr==3.6.0" "pillow>=10.0.0" "nvidia-ml-py>=12.560.30" "pytest>=8.0.0"

    & "$ttsVenv\Scripts\python.exe" -m pip install --upgrade pip
    & "$ttsVenv\Scripts\python.exe" -m pip install "torch==2.8.0" "torchaudio==2.8.0" --index-url "https://download.pytorch.org/whl/cu128"
    & "$ttsVenv\Scripts\python.exe" -m pip install "transformers==4.57.6" "vieneu==3.8.5" "onnxruntime>=1.20.0" "nvidia-ml-py>=12.560.30" "pytest>=8.0.0"
}

# 9. Download local OCR & TTS model assets with user confirmation and record manifest
$modelsRoot = Join-Path $env:LOCALAPPDATA "LumaTranslate\models"
New-Item -ItemType Directory -Force -Path $modelsRoot | Out-Null

if (-not $SkipModelDownload) {
    $proceedDownload = $AcceptModelDownload.IsPresent
    if (-not $proceedDownload) {
        $answer = Read-Host "[9/10] Download PP-OCRv5 Mobile & VieNeu-TTS v3 Turbo models (~950 MB total) to $modelsRoot? [Y/n]"
        $proceedDownload = [string]::IsNullOrWhiteSpace($answer) -or $answer.Trim().StartsWith("Y", [System.StringComparison]::OrdinalIgnoreCase)
    }

    if ($proceedDownload) {
        Write-Host "[9/10] Downloading and warming local OCR & TTS models into $modelsRoot..."
        & "$PSScriptRoot\verify-cuda.ps1" -WriteModelManifest
    } else {
        Write-Warning "Skipped model download; models will be fetched on first worker initialization."
    }
}

Write-Host "[10/10] LumaTranslate setup complete!" -ForegroundColor Green
