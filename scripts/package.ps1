[CmdletBinding()]
param(
    [string]$OutputDir = "dist\LumaTranslate-win-x64",
    [string]$AppVersion = "1.0.0",
    [string]$InstallerBaseName = "LumaTranslate-Setup-x64",
    [switch]$BundleVirtualEnvs,
    [switch]$SkipInstaller,
    [switch]$RequireInstaller
)

$ErrorActionPreference = "Stop"
$RepoRoot = Split-Path -Parent $PSScriptRoot
$DistPath = Join-Path $RepoRoot $OutputDir
$DistParent = Split-Path -Parent $DistPath

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
Copy-Item -Force (Join-Path $RepoRoot "installer\uninstall.ps1") (Join-Path $DistPath "uninstall.ps1")
Copy-Item -Force (Join-Path $RepoRoot "README.md") (Join-Path $DistPath "README.md") -ErrorAction SilentlyContinue
Copy-Item -Force (Join-Path $RepoRoot "README.en.md") (Join-Path $DistPath "README.en.md") -ErrorAction SilentlyContinue
Copy-Item -Force (Join-Path $RepoRoot "LICENSE") (Join-Path $DistPath "LICENSE")

Write-Host "Unpacked distribution staged at: $DistPath" -ForegroundColor Green

# 5. Build per-user AppData installer (.exe) via Inno Setup 6 (ISCC.exe)
if (-not $SkipInstaller) {
    $isccCandidates = @(
        (Get-Command iscc -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source),
        "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe",
        "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
        "$env:ProgramFiles\Inno Setup 6\ISCC.exe"
    ) | Where-Object { $_ -and (Test-Path -LiteralPath $_) }

    $isccExe = $isccCandidates | Select-Object -First 1
    if ($isccExe) {
        $issScript = Join-Path $RepoRoot "installer\LumaTranslate.iss"

        # Ensure LumaTranslate.iss has UTF-8 BOM so ISCC preserves Vietnamese diacritics
        $rawBytes = [System.IO.File]::ReadAllBytes($issScript)
        if ($rawBytes.Length -lt 3 -or $rawBytes[0] -ne 0xEF -or $rawBytes[1] -ne 0xBB -or $rawBytes[2] -ne 0xBF) {
            $utf8WithBom = [System.Text.UTF8Encoding]::new($true)
            $textContent = [System.Text.Encoding]::UTF8.GetString($rawBytes)
            [System.IO.File]::WriteAllText($issScript, $textContent, $utf8WithBom)
        }

        Write-Host "Building Inno Setup per-user AppData installer with: $isccExe" -ForegroundColor Cyan
        & $isccExe `
            "/DAppVersion=$AppVersion" `
            "/DSourceDir=$DistPath" `
            "/DOutputDir=$DistParent" `
            "/DOutputBaseFilename=$InstallerBaseName" `
            $issScript
        if ($LASTEXITCODE -ne 0) {
            throw "Inno Setup compilation (ISCC.exe) failed with exit code $LASTEXITCODE"
        }

        $installerOut = Join-Path $DistParent "$InstallerBaseName.exe"
        Write-Host "Installer package ready at: $installerOut" -ForegroundColor Green
    } elseif ($RequireInstaller) {
        throw "ISCC.exe (Inno Setup 6) not found. Install via: winget install JRSoftware.InnoSetup"
    } else {
        Write-Warning "ISCC.exe (Inno Setup 6) not found — skipped building $InstallerBaseName.exe. Install via 'winget install JRSoftware.InnoSetup' to produce the setup .exe."
    }
}
