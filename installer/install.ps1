<#
.SYNOPSIS
    Optional CLI wrapper invoking LumaTranslate.exe's built-in self-installer.
    End users can simply double-click LumaTranslate.exe directly — if Python 3.12 runtimes
    or local OCR/TTS models are missing, LumaTranslate.exe automatically opens its built-in
    Vietnamese Setup Wizard and launches the app once finished.
#>
[CmdletBinding()]
param(
    [switch]$ExitAfterSetup
)

$ErrorActionPreference = "Stop"
$LauncherExe = Join-Path $PSScriptRoot "LumaTranslate.exe"
if (-not (Test-Path $LauncherExe)) {
    $RepoRoot = Split-Path -Parent $PSScriptRoot
    $LauncherExe = Join-Path $RepoRoot "dist\LumaTranslate-win-x64\LumaTranslate.exe"
}
if (-not (Test-Path $LauncherExe)) {
    throw "LumaTranslate.exe not found. Run scripts\package.ps1 first."
}

$argList = @("--setup", "--auto-install")
if ($ExitAfterSetup) {
    $argList += "--exit-after-setup"
}

Write-Host "Launching LumaTranslate.exe built-in environment installer..." -ForegroundColor Cyan
Start-Process -Wait -FilePath $LauncherExe -ArgumentList $argList -WorkingDirectory (Split-Path $LauncherExe)
