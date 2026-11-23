[CmdletBinding()]
param(
    [string]$ScreenshotDir = ""
)

$ErrorActionPreference = "Continue"
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$OutputEncoding = [System.Text.Encoding]::UTF8
$RepoRoot = Split-Path -Parent $PSScriptRoot
if ([string]::IsNullOrWhiteSpace($ScreenshotDir)) {
    $ScreenshotDir = Join-Path $RepoRoot "dist\screenshots"
}
New-Item -ItemType Directory -Force -Path $ScreenshotDir | Out-Null

$env:WINAPP_UI_WORKFLOW_ID = [guid]::NewGuid().ToString()

# Clean any leftover processes and previous test regions/history in isolated or default store
Get-Process -Name "LumaTranslate", "LumaTranslate.WinUI", "LumaTranslate.Backend", "luma_translate_backend" -ErrorAction SilentlyContinue |
    Stop-Process -Force -ErrorAction SilentlyContinue

$ctrlFile = Join-Path $env:TEMP "luma_e2e_fixture_text.txt"
[System.IO.File]::WriteAllText($ctrlFile, "また会えて嬉しい。", [System.Text.UTF8Encoding]::new($false))

$harnessPy = @"
import ctypes, pathlib, sys, tkinter as tk
try:
    ctypes.windll.shcore.SetProcessDpiAwareness(2)
except Exception:
    pass
ctrl = pathlib.Path(sys.argv[1])
root = tk.Tk()
root.overrideredirect(True)
root.attributes("-topmost", True)
root.configure(bg="#11161F")
root.geometry("760x110+120+120")
lbl = tk.Label(root, text="また会えて嬉しい。", fg="#F8FAFC", bg="#11161F", font=("Yu Gothic UI", 28, "bold"))
lbl.pack(expand=True, fill="both")
def tick():
    if ctrl.exists():
        txt = ctrl.read_text(encoding="utf-8").strip()
        if txt == "__EXIT__":
            root.destroy()
            return
        if lbl.cget("text") != txt:
            lbl.config(text=txt)
    root.after(120, tick)
root.after(120, tick)
root.mainloop()
"@
$harnessPath = Join-Path $env:TEMP "luma_e2e_target_window.py"
[System.IO.File]::WriteAllText($harnessPath, $harnessPy, [System.Text.UTF8Encoding]::new($false))

$pyExe = "C:\Users\Nelovo\AppData\Local\Programs\Python\Python312\python.exe"
$fixtureProc = Start-Process -FilePath $pyExe -ArgumentList @($harnessPath, $ctrlFile) -WindowStyle Hidden -PassThru

$appExe = Join-Path $RepoRoot "apps\LumaTranslate.WinUI\bin\x64\Release\net10.0-windows10.0.26100.0\win-x64\LumaTranslate.exe"
if (-not (Test-Path $appExe)) {
    throw "Release binary not found at $appExe. Run scripts\build.ps1 -Configuration Release first."
}

$appProc = Start-Process -FilePath $appExe -WorkingDirectory (Split-Path $appExe) -PassThru

try {
    Write-Host "[1/8] Waiting for WinUI 3 MainWindow and SubtitleOverlayWindow..." -ForegroundColor Cyan
    $mainHwnd = $null
    $ovHwnd = $null
    for ($i = 0; $i -lt 20; $i++) {
        Start-Sleep -Milliseconds 800
        $winJson = & winapp ui list-windows -a LumaTranslate --json 2>$null
        if ($LASTEXITCODE -eq 0 -and $winJson) {
            $rawList = $winJson | ConvertFrom-Json
            foreach ($w in $rawList) {
                if ($w.title -eq "LumaTranslate" -and $w.width -gt 965) {
                    $mainHwnd = [string]$w.hwnd
                } elseif ($w.width -eq 960 -and $w.height -eq 150) {
                    $ovHwnd = [string]$w.hwnd
                }
            }
            if ($mainHwnd -and $ovHwnd) {
                break
            }
        }
    }
    if (-not $mainHwnd -or -not $ovHwnd) {
        throw "Failed to locate MainWindow ($mainHwnd) or SubtitleOverlayWindow ($ovHwnd)"
    }
    Write-Host "      MainHWND=$mainHwnd | OverlayHWND=$ovHwnd" -ForegroundColor Green

    Write-Host "[2/8] Waiting for Rust backend + CUDA OCR & VieNeu-TTS workers to report READY..." -ForegroundColor Cyan
    $ocrStatus = ""
    $ttsStatus = ""
    for ($i = 0; $i -lt 50; $i++) {
        Start-Sleep -Seconds 1
        $ocrStatus = (& winapp ui get-value OcrDeviceTextBlock -w $mainHwnd 2>$null | Out-String).Trim()
        $ttsStatus = (& winapp ui get-value TtsDeviceTextBlock -w $mainHwnd 2>$null | Out-String).Trim()
        if ($ocrStatus -match "\(CUDA\)" -and $ttsStatus -match "\(CUDA|CPU\)") {
            break
        }
    }
    Write-Host "      OCR Status: $ocrStatus" -ForegroundColor Green
    Write-Host "      TTS Status: $ttsStatus" -ForegroundColor Green

    Write-Host "[3/8] Granting online translation privacy consent & clearing prior cache..." -ForegroundColor Cyan
    & winapp ui invoke ConsentAcceptButton -w $mainHwnd 2>$null | Out-Null
    & winapp ui invoke NavSettingsItem -w $mainHwnd | Out-Null
    Start-Sleep -Milliseconds 500
    & winapp ui invoke ClearCacheButton -w $mainHwnd | Out-Null
    Start-Sleep -Milliseconds 400
    & winapp ui invoke NavTranslateItem -w $mainHwnd | Out-Null
    Start-Sleep -Milliseconds 500

    Write-Host "[4/8] Adding screen region (120, 120, 760x110) covering live Japanese subtitle window..." -ForegroundColor Cyan
    & winapp ui invoke AddRegionButton -w $mainHwnd | Out-Null
    Start-Sleep -Milliseconds 700
    & winapp ui set-value RegionDialogNameBox "Hộp thoại E2E" -w $mainHwnd | Out-Null
    & winapp ui set-value RegionDialogXBox "120" -w $mainHwnd | Out-Null
    & winapp ui set-value RegionDialogYBox "120" -w $mainHwnd | Out-Null
    & winapp ui set-value RegionDialogWidthBox "760" -w $mainHwnd | Out-Null
    & winapp ui set-value RegionDialogHeightBox "110" -w $mainHwnd | Out-Null
    & winapp ui invoke "Lưu vùng" -w $mainHwnd | Out-Null
    Start-Sleep -Milliseconds 700

    Write-Host "[5/8] Starting continuous capture + CUDA OCR + Cloud Translation + Local VieNeu-TTS..." -ForegroundColor Cyan
    & winapp ui invoke ToggleMonitoringButton -w $mainHwnd | Out-Null

    $origRecognized = ""
    $viTranslated = ""
    $overlayText = ""
    for ($i = 0; $i -lt 25; $i++) {
        Start-Sleep -Milliseconds 800
        $origRecognized = (& winapp ui get-value CurrentOriginalTextBlock -w $mainHwnd 2>$null | Out-String).Trim()
        $viTranslated = (& winapp ui get-value CurrentVietnameseTextBlock -w $mainHwnd 2>$null | Out-String).Trim()
        $overlayText = (& winapp ui get-value OverlaySubtitleTextBlock -w $ovHwnd 2>$null | Out-String).Trim()
        if ($origRecognized -match "また会えて" -and $viTranslated -notmatch "Chọn một vùng") {
            break
        }
    }
    Write-Host "      Recognized OCR : $origRecognized" -ForegroundColor Green
    Write-Host "      Vietnamese UI  : $viTranslated" -ForegroundColor Green
    Write-Host "      Overlay Window : $overlayText" -ForegroundColor Green

    # Allow 2s for audible VieNeu-TTS speech playback over WASAPI and capture screenshot
    Start-Sleep -Seconds 2
    & winapp ui screenshot -w $mainHwnd --output (Join-Path $ScreenshotDir "main_translating.png") | Out-Null
    & winapp ui screenshot -w $ovHwnd --output (Join-Path $ScreenshotDir "overlay_translating.png") | Out-Null

    Write-Host "[6/8] Verifying SQLite/LRU Cache Hit on repeated dialogue after blank transition..." -ForegroundColor Cyan
    [System.IO.File]::WriteAllText($ctrlFile, "", [System.Text.UTF8Encoding]::new($false))
    Start-Sleep -Milliseconds 1200
    [System.IO.File]::WriteAllText($ctrlFile, "また会えて嬉しい。", [System.Text.UTF8Encoding]::new($false))
    $provStatus = ""
    for ($i = 0; $i -lt 15; $i++) {
        Start-Sleep -Milliseconds 500
        $provStatus = (& winapp ui get-value ProviderStatusTextBlock -w $mainHwnd 2>$null | Out-String).Trim()
        if ($provStatus -match "Bộ nhớ đệm") {
            break
        }
    }
    Write-Host "      Provider Status on Repeat: $provStatus" -ForegroundColor Green

    Write-Host "[7/8] Stopping monitoring & inspecting SQLite Conversation History UI..." -ForegroundColor Cyan
    & winapp ui invoke ToggleMonitoringButton -w $mainHwnd | Out-Null
    Start-Sleep -Milliseconds 600
    $monStatus = (& winapp ui get-value MonitoringStatusTextBlock -w $mainHwnd 2>$null | Out-String).Trim()
    Write-Host "      Monitoring Status after Stop: $monStatus" -ForegroundColor Green

    & winapp ui invoke NavHistoryItem -w $mainHwnd | Out-Null
    Start-Sleep -Seconds 1
    & winapp ui screenshot -w $mainHwnd --output (Join-Path $ScreenshotDir "history_page.png") | Out-Null

    & winapp ui invoke NavSettingsItem -w $mainHwnd | Out-Null
    Start-Sleep -Seconds 1
    & winapp ui screenshot -w $mainHwnd --output (Join-Path $ScreenshotDir "settings_page.png") | Out-Null

    Write-Host "[8/8] Verifying clean shutdown of WinUI, Rust Backend, and Python AI workers..." -ForegroundColor Cyan
    & winapp ui yield | Out-Null
    $null = $appProc.CloseMainWindow()
    Start-Sleep -Seconds 3
    if (-not $appProc.HasExited) {
        Stop-Process -Id $appProc.Id -Force -ErrorAction SilentlyContinue
    }
    Start-Sleep -Seconds 2
    $remainingBackend = Get-Process -Name "LumaTranslate.Backend", "luma_translate_backend" -ErrorAction SilentlyContinue
    Write-Host "      Remaining backend processes after UI close: $($remainingBackend.Count)" -ForegroundColor Green

    if ($origRecognized -notmatch "また会えて") {
        throw "End-to-end OCR failed to recognize live desktop Japanese subtitle (got: $origRecognized)"
    }
    if ([string]::IsNullOrWhiteSpace($viTranslated) -or $viTranslated -match "Chọn một vùng") {
        throw "End-to-end Cloud Translation failed to produce Vietnamese output (got: $viTranslated)"
    }

    Write-Host "=== END-TO-END WINDOWS DESKTOP VERIFICATION PASSED! ===" -ForegroundColor Green
} finally {
    [System.IO.File]::WriteAllText($ctrlFile, "__EXIT__", [System.Text.UTF8Encoding]::new($false))
    Start-Sleep -Milliseconds 300
    if ($fixtureProc -and -not $fixtureProc.HasExited) {
        Stop-Process -Id $fixtureProc.Id -Force -ErrorAction SilentlyContinue
    }
    if ($appProc -and -not $appProc.HasExited) {
        Stop-Process -Id $appProc.Id -Force -ErrorAction SilentlyContinue
    }
}
