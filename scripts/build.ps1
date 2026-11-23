[CmdletBinding()]
param(
    [ValidateSet("Debug", "Release")]
    [string]$Configuration = "Release"
)

$ErrorActionPreference = "Stop"
$RepoRoot = Split-Path -Parent $PSScriptRoot

Get-Process -Name "LumaTranslate.WinUI", "LumaTranslate", "LumaTranslate.Backend", "luma_translate_backend" -ErrorAction SilentlyContinue |
    Stop-Process -Force -ErrorAction SilentlyContinue

Write-Host "=== [1/2] Building Rust Backend ($Configuration) ===" -ForegroundColor Cyan
Push-Location (Join-Path $RepoRoot "backend")
try {
    if ($Configuration -eq "Release") {
        cargo build --release
        if ($LASTEXITCODE -ne 0) { throw "cargo build --release failed" }
        Copy-Item -Force "target\release\luma_translate_backend.exe" "target\release\LumaTranslate.Backend.exe"
    } else {
        cargo build
        if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
        Copy-Item -Force "target\debug\luma_translate_backend.exe" "target\debug\LumaTranslate.Backend.exe"
    }
} finally {
    Pop-Location
}

Write-Host "=== [2/2] Building WinUI 3 Application ($Configuration | x64) ===" -ForegroundColor Cyan
$csproj = Join-Path $RepoRoot "apps\LumaTranslate.WinUI\LumaTranslate.WinUI.csproj"
dotnet build $csproj -c $Configuration -p:Platform=x64
if ($LASTEXITCODE -ne 0) {
    throw "dotnet build failed for LumaTranslate.WinUI"
}

$winuiOut = Get-ChildItem -Path (Join-Path $RepoRoot "apps\LumaTranslate.WinUI\bin") -Recurse -Filter "LumaTranslate.exe" |
    Sort-Object LastWriteTime -Descending |
    Select-Object -First 1

if ($null -ne $winuiOut) {
    $backendProfile = if ($Configuration -eq "Release") { "release" } else { "debug" }
    $backendExe = Join-Path $RepoRoot "backend\target\$backendProfile\LumaTranslate.Backend.exe"
    Copy-Item -Force $backendExe (Join-Path $winuiOut.DirectoryName "LumaTranslate.Backend.exe")
    Write-Host "Built WinUI executable: $($winuiOut.FullName)" -ForegroundColor Green
}
