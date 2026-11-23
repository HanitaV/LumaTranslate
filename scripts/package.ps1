[CmdletBinding()]
param(
    [string]$OutputDir = "dist\LumaTranslate-win-x64",
    [switch]$BundleVirtualEnvs
)

$ErrorActionPreference = "Stop"
$RepoRoot = Split-Path -Parent $PSScriptRoot
$DistPath = Join-Path $RepoRoot $OutputDir

Write-Host "=== Packaging LumaTranslate Windows x64 Distribution ===" -ForegroundColor Cyan

# 1. Build Rust release binary
Push-Location (Join-Path $RepoRoot "backend")
try {
    cargo build --release
    if ($LASTEXITCODE -ne 0) { throw "cargo build --release failed" }
} finally {
    Pop-Location
}

# 2. Publish unpackaged self-contained WinUI 3 x64 app
if (Test-Path $DistPath) {
    Remove-Item -Recurse -Force $DistPath
}
New-Item -ItemType Directory -Force -Path $DistPath | Out-Null

$csproj = Join-Path $RepoRoot "apps\LumaTranslate.WinUI\LumaTranslate.WinUI.csproj"
dotnet publish $csproj `
    -c Release `
    -r win-x64 `
    -p:Platform=x64 `
    -p:WindowsAppSDKSelfContained=true `
    --self-contained true `
    -p:PublishTrimmed=false `
    -o $DistPath
if ($LASTEXITCODE -ne 0) {
    throw "dotnet publish failed for LumaTranslate.WinUI"
}

# Remove legacy duplicate check — <AssemblyName>LumaTranslate</AssemblyName> produces LumaTranslate.exe directly
if (-not (Test-Path (Join-Path $DistPath "LumaTranslate.exe"))) {
    throw "Expected LumaTranslate.exe was not produced in $DistPath"
}

# 3. Copy Rust backend binary
$backendBin = Join-Path $RepoRoot "backend\target\release\luma_translate_backend.exe"
Copy-Item -Force $backendBin (Join-Path $DistPath "LumaTranslate.Backend.exe")

# 4. Copy worker scripts & installer bootstrap (users never activate Python envs manually)
$workersDest = Join-Path $DistPath "workers"
New-Item -ItemType Directory -Force -Path "$workersDest\ocr", "$workersDest\tts", "$workersDest\shared" | Out-Null
Copy-Item -Force (Join-Path $RepoRoot "workers\shared\protocol.py") "$workersDest\shared\protocol.py"
Copy-Item -Force (Join-Path $RepoRoot "workers\ocr\*.py") "$workersDest\ocr\"
Copy-Item -Force (Join-Path $RepoRoot "workers\ocr\requirements.txt") "$workersDest\ocr\requirements.txt"
Copy-Item -Force (Join-Path $RepoRoot "workers\tts\*.py") "$workersDest\tts\"
Copy-Item -Force (Join-Path $RepoRoot "workers\tts\requirements.txt") "$workersDest\tts\requirements.txt"

if ($BundleVirtualEnvs) {
    Write-Host "Copying local OCR & TTS .venv runtimes into distribution..."
    Copy-Item -Recurse -Force (Join-Path $RepoRoot "workers\ocr\.venv") "$workersDest\ocr\.venv"
    Copy-Item -Recurse -Force (Join-Path $RepoRoot "workers\tts\.venv") "$workersDest\tts\.venv"
}

Copy-Item -Force (Join-Path $RepoRoot "installer\install.ps1") (Join-Path $DistPath "install-runtimes.ps1")
Copy-Item -Force (Join-Path $RepoRoot "README.md") (Join-Path $DistPath "README.md") -ErrorAction SilentlyContinue
Copy-Item -Force (Join-Path $RepoRoot "LICENSE") (Join-Path $DistPath "LICENSE")

Write-Host "Packaged distribution ready at: $DistPath" -ForegroundColor Green
