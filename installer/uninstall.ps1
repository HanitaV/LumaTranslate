<#
.SYNOPSIS
    Gỡ cài đặt LumaTranslate (môi trường Python 3.12 cục bộ, mô hình AI, dữ liệu người dùng,
    mục đăng ký Windows và tùy chọn xóa thư mục bộ cài).
.DESCRIPTION
    - Chạy trực tiếp không tham số: mở hộp thoại xác nhận gỡ cài đặt tiếng Việt của LumaTranslate.exe
      (nếu tìm thấy tệp thực thi), hoặc gỡ cài đặt trực tiếp qua PowerShell.
    - Chạy tự động (không hộp thoại): truyền -Unattended (tùy chọn kèm -KeepUserData, -KeepAiEnvironment, -RemoveAppFolder).
#>
[CmdletBinding()]
param(
    [switch]$Unattended,
    [switch]$KeepUserData,
    [switch]$KeepAiEnvironment,
    [switch]$RemoveAppFolder,
    [switch]$SkipRegistryAndShortcuts,
    [string]$LocalDataRoot = ""
)

$ErrorActionPreference = "Stop"
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

$AppDir = $PSScriptRoot
$LauncherExe = Join-Path $AppDir "LumaTranslate.exe"
if (-not (Test-Path -LiteralPath $LauncherExe)) {
    $RepoCandidate = Split-Path -Parent $PSScriptRoot
    $DistCandidate = Join-Path $RepoCandidate "dist\LumaTranslate-win-x64\LumaTranslate.exe"
    if (Test-Path -LiteralPath $DistCandidate) {
        $LauncherExe = $DistCandidate
        $AppDir = Split-Path -Parent $DistCandidate
    }
}

if (-not $Unattended -and [string]::IsNullOrWhiteSpace($LocalDataRoot) -and (Test-Path -LiteralPath $LauncherExe)) {
    Write-Host "Đang mở trình gỡ cài đặt giao diện LumaTranslate..." -ForegroundColor Cyan
    Start-Process -Wait -FilePath $LauncherExe -ArgumentList @("--uninstall") -WorkingDirectory (Split-Path -Parent $LauncherExe)
    exit 0
}

Write-Host "=== Đang gỡ cài đặt LumaTranslate ===" -ForegroundColor Cyan

$LocalRoot = if ([string]::IsNullOrWhiteSpace($LocalDataRoot)) {
    Join-Path $env:LOCALAPPDATA "LumaTranslate"
} else {
    $LocalDataRoot
}

# 1. Dừng các tiến trình đang chạy của LumaTranslate (chỉ dừng tiến trình hệ thống khi gỡ cài đặt thật)
if ([string]::IsNullOrWhiteSpace($LocalDataRoot)) {
    Get-Process -Name "LumaTranslate", "LumaTranslate.WinUI", "LumaTranslate.Backend", "luma_translate_backend" -ErrorAction SilentlyContinue |
        Stop-Process -Force -ErrorAction SilentlyContinue

    Get-Process -Name "python" -ErrorAction SilentlyContinue | ForEach-Object {
        try {
            $modPath = $_.MainModule.FileName
            if ($modPath -and (
                $modPath.StartsWith($LocalRoot, [System.StringComparison]::OrdinalIgnoreCase) -or
                $modPath.StartsWith($AppDir, [System.StringComparison]::OrdinalIgnoreCase))) {
                Stop-Process -Id $_.Id -Force -ErrorAction SilentlyContinue
            }
        } catch {
        }
    }

    Start-Sleep -Milliseconds 400
}

# 2. Xóa mục đăng ký Windows Installed Apps & lối tắt
if (-not $SkipRegistryAndShortcuts) {
    foreach ($regSub in @(
        "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\LumaTranslate",
        "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\LumaTranslate_is1"
    )) {
        if (Test-Path -LiteralPath $regSub) {
            Remove-Item -LiteralPath $regSub -Recurse -Force -ErrorAction SilentlyContinue
            Write-Host "Đã xóa mục đăng ký ứng dụng trong Windows ($regSub)." -ForegroundColor Green
        }
    }

    $ShortcutPaths = @(
        (Join-Path ([Environment]::GetFolderPath("DesktopDirectory")) "LumaTranslate.lnk"),
        (Join-Path ([Environment]::GetFolderPath("Programs")) "LumaTranslate.lnk")
    )
    foreach ($lnk in $ShortcutPaths) {
        if (Test-Path -LiteralPath $lnk) {
            Remove-Item -LiteralPath $lnk -Force -ErrorAction SilentlyContinue
        }
    }
    $StartMenuDir = Join-Path ([Environment]::GetFolderPath("Programs")) "LumaTranslate"
    if (Test-Path -LiteralPath $StartMenuDir) {
        Remove-Item -LiteralPath $StartMenuDir -Recurse -Force -ErrorAction SilentlyContinue
    }
}

# 3. Xóa môi trường AI cục bộ (%LOCALAPPDATA%\LumaTranslate\runtimes, models, tools)
if (-not $KeepAiEnvironment) {
    foreach ($sub in @("runtimes", "models", "tools")) {
        $target = Join-Path $LocalRoot $sub
        if (Test-Path -LiteralPath $target) {
            Write-Host "Đang xóa $target ..."
            Remove-Item -LiteralPath $target -Recurse -Force -ErrorAction SilentlyContinue
        }
    }
}

# 4. Xóa dữ liệu người dùng (%LOCALAPPDATA%\LumaTranslate\data)
if (-not $KeepUserData) {
    $dataDir = Join-Path $LocalRoot "data"
    if (Test-Path -LiteralPath $dataDir) {
        Write-Host "Đang xóa dữ liệu người dùng $dataDir ..."
        Remove-Item -LiteralPath $dataDir -Recurse -Force -ErrorAction SilentlyContinue
    }
    $uiLog = Join-Path $LocalRoot "ui-errors.log"
    if (Test-Path -LiteralPath $uiLog) {
        Remove-Item -LiteralPath $uiLog -Force -ErrorAction SilentlyContinue
    }
}

if (Test-Path -LiteralPath $LocalRoot) {
    $remaining = Get-ChildItem -LiteralPath $LocalRoot -Force -ErrorAction SilentlyContinue
    if (-not $remaining -or (-not $KeepAiEnvironment -and -not $KeepUserData)) {
        Remove-Item -LiteralPath $LocalRoot -Recurse -Force -ErrorAction SilentlyContinue
    }
}

# 5. Xóa thư mục bộ cài ứng dụng (nếu được yêu cầu và không nằm trong kho mã nguồn .git)
if ($RemoveAppFolder -and (Test-Path -LiteralPath (Join-Path $AppDir "LumaTranslate.exe"))) {
    $inGitRepo = $false
    $cursor = Get-Item -LiteralPath $AppDir
    while ($null -ne $cursor) {
        if ((Test-Path -LiteralPath (Join-Path $cursor.FullName ".git")) -or
            (Test-Path -LiteralPath (Join-Path $cursor.FullName "LumaTranslate.WinUI.csproj"))) {
            $inGitRepo = $true
            break
        }
        $cursor = $cursor.Parent
    }

    if ($inGitRepo) {
        Write-Warning "Bỏ qua xóa thư mục '$AppDir' vì đang nằm trong kho mã nguồn phát triển (.git)."
    } else {
        Write-Host "Đang lên lịch xóa thư mục bộ cài '$AppDir'..." -ForegroundColor Yellow
        $escapedAppDir = $AppDir.Replace("'", "''")
        $cleanupScript = "Start-Sleep -Milliseconds 800; Remove-Item -LiteralPath '$escapedAppDir' -Recurse -Force -ErrorAction SilentlyContinue"
        $encoded = [Convert]::ToBase64String([System.Text.Encoding]::Unicode.GetBytes($cleanupScript))
        Start-Process -FilePath "powershell.exe" `
            -WorkingDirectory ([System.IO.Path]::GetTempPath()) `
            -WindowStyle Hidden `
            -ArgumentList @("-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-EncodedCommand", $encoded) | Out-Null
    }
}

Write-Host "Hoàn tất gỡ cài đặt LumaTranslate." -ForegroundColor Green
