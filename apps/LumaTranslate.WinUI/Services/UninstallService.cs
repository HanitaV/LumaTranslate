using System.Diagnostics;
using System.Text;
using Microsoft.Win32;

namespace LumaTranslate_WinUI.Services;

public sealed record UninstallInspection(
    string AppDataRoot,
    string AppBaseDirectory,
    long AiEnvironmentBytes,
    long UserDataBytes,
    bool HasAiEnvironment,
    bool HasUserData,
    bool HasWindowsUninstallEntry,
    bool CanDeleteAppFolder,
    string? CannotDeleteAppFolderReasonVi);

public sealed record UninstallOptions(
    bool RemoveAiEnvironment,
    bool RemoveUserData,
    bool RemoveShortcutsAndRegistry,
    bool RemoveAppFolder);

public static class UninstallService
{
    private const string UninstallRegistrySubKey =
        @"Software\Microsoft\Windows\CurrentVersion\Uninstall\LumaTranslate";

    private static (DateTimeOffset Timestamp, UninstallInspection Value)? _cachedInspection;

    public static UninstallInspection InspectQuick()
    {
        if (_cachedInspection is { } cached &&
            DateTimeOffset.UtcNow - cached.Timestamp < TimeSpan.FromSeconds(45))
        {
            return cached.Value;
        }

        string appDataRoot = EnvironmentBootstrapService.AppLocalDataRoot;
        string baseDir = NormalizeDirectoryPath(AppContext.BaseDirectory);

        bool hasAiEnv =
            Directory.Exists(EnvironmentBootstrapService.ManagedRuntimesRoot) ||
            Directory.Exists(EnvironmentBootstrapService.ManagedModelsRoot) ||
            Directory.Exists(EnvironmentBootstrapService.ManagedToolsRoot);

        string dataDir = Path.Combine(appDataRoot, "data");
        string uiLogFile = Path.Combine(appDataRoot, "ui-errors.log");
        bool hasUserData = Directory.Exists(dataDir) || File.Exists(uiLogFile);

        bool hasRegEntry = HasWindowsUninstallRegistration();
        var (canDeleteAppFolder, reasonVi) = EvaluateAppFolderDeletionSafety(baseDir);

        return new UninstallInspection(
            appDataRoot,
            baseDir,
            AiEnvironmentBytes: -1,
            UserDataBytes: -1,
            hasAiEnv,
            hasUserData,
            hasRegEntry,
            canDeleteAppFolder,
            reasonVi);
    }

    public static async Task<UninstallInspection> InspectAsync(
        bool forceRefresh = false,
        CancellationToken ct = default)
    {
        if (!forceRefresh &&
            _cachedInspection is { } cached &&
            DateTimeOffset.UtcNow - cached.Timestamp < TimeSpan.FromSeconds(45))
        {
            return cached.Value;
        }

        UninstallInspection computed = await Task.Run(() => InspectSync(ct), ct).ConfigureAwait(false);
        _cachedInspection = (DateTimeOffset.UtcNow, computed);
        return computed;
    }

    public static void InvalidateCache() => _cachedInspection = null;

    private static UninstallInspection InspectSync(CancellationToken ct)
    {
        string appDataRoot = EnvironmentBootstrapService.AppLocalDataRoot;
        string baseDir = NormalizeDirectoryPath(AppContext.BaseDirectory);

        string[] aiDirs =
        [
            EnvironmentBootstrapService.ManagedRuntimesRoot,
            EnvironmentBootstrapService.ManagedModelsRoot,
            EnvironmentBootstrapService.ManagedToolsRoot,
        ];

        long aiBytes = 0;
        bool hasAiEnv = false;
        foreach (string dir in aiDirs)
        {
            ct.ThrowIfCancellationRequested();
            if (Directory.Exists(dir))
            {
                hasAiEnv = true;
                aiBytes += CalculateDirectorySizeSafe(dir, ct);
            }
        }

        string dataDir = Path.Combine(appDataRoot, "data");
        string uiLogFile = Path.Combine(appDataRoot, "ui-errors.log");
        long userDataBytes = 0;
        bool hasUserData = false;

        if (Directory.Exists(dataDir))
        {
            hasUserData = true;
            userDataBytes += CalculateDirectorySizeSafe(dataDir, ct);
        }

        if (File.Exists(uiLogFile))
        {
            hasUserData = true;
            try
            {
                userDataBytes += new FileInfo(uiLogFile).Length;
            }
            catch
            {
            }
        }

        bool hasRegEntry = HasWindowsUninstallRegistration();
        var (canDeleteAppFolder, reasonVi) = EvaluateAppFolderDeletionSafety(baseDir);

        return new UninstallInspection(
            appDataRoot,
            baseDir,
            aiBytes,
            userDataBytes,
            hasAiEnv,
            hasUserData,
            hasRegEntry,
            canDeleteAppFolder,
            reasonVi);
    }

    public static (bool CanDelete, string? ReasonVi) EvaluateAppFolderDeletionSafety(string appBaseDirectory)
    {
        if (string.IsNullOrWhiteSpace(appBaseDirectory))
        {
            return (false, "Không xác định được thư mục chứa ứng dụng hiện tại.");
        }

        string normalized = NormalizeDirectoryPath(appBaseDirectory);
        var dirInfo = new DirectoryInfo(normalized);
        if (!dirInfo.Exists)
        {
            return (false, "Thư mục ứng dụng không tồn tại.");
        }

        if (dirInfo.Parent is null)
        {
            return (false, "Không cho phép xóa thư mục gốc của ổ đĩa.");
        }

        if (IsInsideSourceRepository(normalized))
        {
            return (false, "Đang chạy từ thư mục mã nguồn phát triển (.git) — khóa tự xóa thư mục ứng dụng để bảo vệ mã nguồn.");
        }

        Environment.SpecialFolder[] protectedFolders =
        [
            Environment.SpecialFolder.Windows,
            Environment.SpecialFolder.System,
            Environment.SpecialFolder.ProgramFiles,
            Environment.SpecialFolder.ProgramFilesX86,
            Environment.SpecialFolder.UserProfile,
            Environment.SpecialFolder.DesktopDirectory,
            Environment.SpecialFolder.MyDocuments,
            Environment.SpecialFolder.LocalApplicationData,
            Environment.SpecialFolder.ApplicationData,
        ];

        foreach (Environment.SpecialFolder sf in protectedFolders)
        {
            string specialPath = Environment.GetFolderPath(sf);
            if (!string.IsNullOrWhiteSpace(specialPath) &&
                string.Equals(NormalizeDirectoryPath(specialPath), normalized, StringComparison.OrdinalIgnoreCase))
            {
                return (false, "Thư mục hiện tại là thư mục hệ thống của Windows — không thể xóa toàn bộ thư mục này.");
            }
        }

        string expectedExe = Path.Combine(normalized, "LumaTranslate.exe");
        if (!File.Exists(expectedExe))
        {
            return (false, "Không tìm thấy LumaTranslate.exe trong thư mục hiện tại.");
        }

        return (true, null);
    }

    public static bool IsInsideSourceRepository(string directoryPath)
    {
        try
        {
            DirectoryInfo? cursor = new(NormalizeDirectoryPath(directoryPath));
            while (cursor is not null)
            {
                if (Directory.Exists(Path.Combine(cursor.FullName, ".git")) ||
                    File.Exists(Path.Combine(cursor.FullName, ".git")) ||
                    File.Exists(Path.Combine(cursor.FullName, "LumaTranslate.WinUI.csproj")) ||
                    (File.Exists(Path.Combine(cursor.FullName, "AGENTS.md")) &&
                     Directory.Exists(Path.Combine(cursor.FullName, "backend"))))
                {
                    return true;
                }
                cursor = cursor.Parent;
            }
        }
        catch
        {
        }

        return false;
    }

    public static bool HasWindowsUninstallRegistration()
    {
        try
        {
            using RegistryKey? key = Registry.CurrentUser.OpenSubKey(UninstallRegistrySubKey, writable: false);
            return key is not null;
        }
        catch
        {
            return false;
        }
    }

    public static void RegisterWindowsUninstaller(bool allowDevBuild = false)
    {
        try
        {
            string baseDir = NormalizeDirectoryPath(AppContext.BaseDirectory);
            if (!allowDevBuild && IsInsideSourceRepository(baseDir))
            {
                return;
            }

            string exePath = Path.Combine(baseDir, "LumaTranslate.exe");
            if (!File.Exists(exePath))
            {
                return;
            }

            string iconPath = Path.Combine(baseDir, "Assets", "AppIcon.ico");
            if (!File.Exists(iconPath))
            {
                iconPath = exePath;
            }

            using RegistryKey key = Registry.CurrentUser.CreateSubKey(UninstallRegistrySubKey, writable: true);
            key.SetValue("DisplayName", "LumaTranslate", RegistryValueKind.String);
            key.SetValue("DisplayVersion", "1.0.0", RegistryValueKind.String);
            key.SetValue("Publisher", "LumaTranslate", RegistryValueKind.String);
            key.SetValue("DisplayIcon", iconPath, RegistryValueKind.String);
            key.SetValue("InstallLocation", baseDir, RegistryValueKind.String);
            key.SetValue("UninstallString", $"\"{exePath}\" --uninstall", RegistryValueKind.String);
            key.SetValue("ModifyPath", $"\"{exePath}\" --setup", RegistryValueKind.String);
            key.SetValue("NoModify", 0, RegistryValueKind.DWord);
            key.SetValue("NoRepair", 1, RegistryValueKind.DWord);
        }
        catch
        {
        }
    }

    public static void RemoveShortcutsAndRegistry()
    {
        try
        {
            Registry.CurrentUser.DeleteSubKeyTree(UninstallRegistrySubKey, throwOnMissingSubKey: false);
        }
        catch
        {
        }

        string[] shortcutCandidates =
        [
            Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.DesktopDirectory), "LumaTranslate.lnk"),
            Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.Programs), "LumaTranslate.lnk"),
        ];

        foreach (string lnk in shortcutCandidates)
        {
            TryDeleteFile(lnk);
        }

        string startMenuFolder = Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.Programs),
            "LumaTranslate");
        if (Directory.Exists(startMenuFolder))
        {
            TryDeleteDirectoryImmediate(startMenuFolder);
        }
    }

    public static async Task ExecuteUninstallAsync(
        UninstallOptions options,
        Func<Task>? gracefulShutdownAsync = null,
        IProgress<string>? statusProgress = null)
    {
        statusProgress?.Report("Đang dừng tiến trình nền và các tác vụ AI...");

        if (gracefulShutdownAsync is not null)
        {
            try
            {
                await gracefulShutdownAsync().ConfigureAwait(false);
            }
            catch
            {
            }
        }

        string appDataRoot = NormalizeDirectoryPath(EnvironmentBootstrapService.AppLocalDataRoot);
        string baseDir = NormalizeDirectoryPath(AppContext.BaseDirectory);

        await Task.Run(async () =>
        {
            TerminateRelatedProcesses(appDataRoot, baseDir);
            await Task.Delay(300).ConfigureAwait(false);

            var deferredPaths = new List<string>();

            if (options.RemoveShortcutsAndRegistry)
            {
                statusProgress?.Report("Đang gỡ lối tắt và mục đăng ký ứng dụng Windows...");
                RemoveShortcutsAndRegistry();
            }

            if (options.RemoveAiEnvironment)
            {
                statusProgress?.Report("Đang xóa môi trường Python 3.12 và các mô hình AI cục bộ...");
                string[] aiDirs =
                [
                    EnvironmentBootstrapService.ManagedRuntimesRoot,
                    EnvironmentBootstrapService.ManagedModelsRoot,
                    EnvironmentBootstrapService.ManagedToolsRoot,
                ];
                foreach (string dir in aiDirs)
                {
                    if (Directory.Exists(dir) && !TryDeleteDirectoryImmediate(dir))
                    {
                        deferredPaths.Add(NormalizeDirectoryPath(dir));
                    }
                }
            }

            if (options.RemoveUserData)
            {
                statusProgress?.Report("Đang xóa cài đặt, bộ nhớ đệm và lịch sử hội thoại...");
                string dataDir = Path.Combine(appDataRoot, "data");
                if (Directory.Exists(dataDir) && !TryDeleteDirectoryImmediate(dataDir))
                {
                    deferredPaths.Add(NormalizeDirectoryPath(dataDir));
                }

                string uiLog = Path.Combine(appDataRoot, "ui-errors.log");
                if (File.Exists(uiLog) && !TryDeleteFile(uiLog))
                {
                    deferredPaths.Add(uiLog);
                }
            }

            if (Directory.Exists(appDataRoot))
            {
                bool removeEntireAppData = options.RemoveAiEnvironment && options.RemoveUserData;
                bool isEmpty = false;
                try
                {
                    isEmpty = !Directory.EnumerateFileSystemEntries(appDataRoot).Any();
                }
                catch
                {
                }

                if (removeEntireAppData || isEmpty)
                {
                    if (!TryDeleteDirectoryImmediate(appDataRoot))
                    {
                        deferredPaths.Add(appDataRoot);
                    }
                }
            }

            if (options.RemoveAppFolder)
            {
                var (canDelete, _) = EvaluateAppFolderDeletionSafety(baseDir);
                if (canDelete)
                {
                    deferredPaths.Add(baseDir);
                }
            }

            if (deferredPaths.Count > 0)
            {
                statusProgress?.Report("Đang lên lịch dọn dẹp tệp thực thi sau khi đóng ứng dụng...");
                SchedulePostExitCleanup(Environment.ProcessId, deferredPaths);
            }
        }).ConfigureAwait(false);
    }

    public static void OpenAppDataFolderInExplorer()
    {
        string dir = EnvironmentBootstrapService.AppLocalDataRoot;
        Directory.CreateDirectory(dir);
        Process.Start(new ProcessStartInfo
        {
            FileName = "explorer.exe",
            Arguments = $"\"{dir}\"",
            UseShellExecute = true,
        });
    }

    public static string FormatBytesVi(long bytes)
    {
        if (bytes <= 0)
        {
            return "0 MB";
        }

        const double kb = 1024.0;
        const double mb = kb * 1024.0;
        const double gb = mb * 1024.0;

        if (bytes >= gb)
        {
            return $"{bytes / gb:F2} GB";
        }
        if (bytes >= mb)
        {
            return $"{bytes / mb:F1} MB";
        }
        return $"{Math.Max(1.0, bytes / kb):F0} KB";
    }

    private static void TerminateRelatedProcesses(string appDataRoot, string appBaseDirectory)
    {
        int currentPid = Environment.ProcessId;

        foreach (string procName in new[] { "LumaTranslate.Backend", "luma_translate_backend" })
        {
            try
            {
                foreach (Process p in Process.GetProcessesByName(procName))
                {
                    using (p)
                    {
                        if (p.Id == currentPid)
                        {
                            continue;
                        }
                        try
                        {
                            p.Kill(entireProcessTree: true);
                        }
                        catch
                        {
                        }
                    }
                }
            }
            catch
            {
            }
        }

        try
        {
            foreach (Process p in Process.GetProcessesByName("python"))
            {
                using (p)
                {
                    try
                    {
                        string? mainModulePath = p.MainModule?.FileName;
                        if (!string.IsNullOrWhiteSpace(mainModulePath) &&
                            (mainModulePath.StartsWith(appDataRoot, StringComparison.OrdinalIgnoreCase) ||
                             mainModulePath.StartsWith(appBaseDirectory, StringComparison.OrdinalIgnoreCase)))
                        {
                            p.Kill(entireProcessTree: true);
                        }
                    }
                    catch
                    {
                    }
                }
            }
        }
        catch
        {
        }
    }

    private static void SchedulePostExitCleanup(int currentPid, IReadOnlyList<string> pathsToRemove)
    {
        var sb = new StringBuilder();
        sb.AppendLine("$ErrorActionPreference = 'SilentlyContinue'");
        sb.AppendLine($"Wait-Process -Id {currentPid} -Timeout 12");
        sb.AppendLine("Start-Sleep -Milliseconds 600");

        foreach (string rawPath in pathsToRemove.Distinct(StringComparer.OrdinalIgnoreCase))
        {
            string escaped = rawPath.Replace("'", "''");
            sb.AppendLine($"for ($i = 0; $i -lt 5; $i++) {{");
            sb.AppendLine($"    if (-not (Test-Path -LiteralPath '{escaped}')) {{ break }}");
            sb.AppendLine($"    Remove-Item -LiteralPath '{escaped}' -Recurse -Force -ErrorAction SilentlyContinue");
            sb.AppendLine($"    Start-Sleep -Milliseconds 500");
            sb.AppendLine($"}}");
        }

        string encodedCommand = Convert.ToBase64String(Encoding.Unicode.GetBytes(sb.ToString()));
        var psi = new ProcessStartInfo
        {
            FileName = "powershell.exe",
            WorkingDirectory = Path.GetTempPath(),
            UseShellExecute = false,
            CreateNoWindow = true,
        };
        psi.ArgumentList.Add("-NoProfile");
        psi.ArgumentList.Add("-NonInteractive");
        psi.ArgumentList.Add("-ExecutionPolicy");
        psi.ArgumentList.Add("Bypass");
        psi.ArgumentList.Add("-WindowStyle");
        psi.ArgumentList.Add("Hidden");
        psi.ArgumentList.Add("-EncodedCommand");
        psi.ArgumentList.Add(encodedCommand);

        try
        {
            Process.Start(psi);
        }
        catch
        {
        }
    }

    private static bool TryDeleteDirectoryImmediate(string dirPath)
    {
        try
        {
            if (!Directory.Exists(dirPath))
            {
                return true;
            }

            foreach (string file in Directory.EnumerateFiles(dirPath, "*", SearchOption.AllDirectories))
            {
                try
                {
                    FileAttributes attrs = File.GetAttributes(file);
                    if ((attrs & FileAttributes.ReadOnly) != 0)
                    {
                        File.SetAttributes(file, attrs & ~FileAttributes.ReadOnly);
                    }
                }
                catch
                {
                }
            }

            Directory.Delete(dirPath, recursive: true);
            return !Directory.Exists(dirPath);
        }
        catch
        {
            return false;
        }
    }

    private static bool TryDeleteFile(string filePath)
    {
        try
        {
            if (!File.Exists(filePath))
            {
                return true;
            }
            FileAttributes attrs = File.GetAttributes(filePath);
            if ((attrs & FileAttributes.ReadOnly) != 0)
            {
                File.SetAttributes(filePath, attrs & ~FileAttributes.ReadOnly);
            }
            File.Delete(filePath);
            return !File.Exists(filePath);
        }
        catch
        {
            return false;
        }
    }

    private static long CalculateDirectorySizeSafe(string dirPath, CancellationToken ct)
    {
        long total = 0;
        var pending = new Stack<string>();
        pending.Push(dirPath);

        while (pending.Count > 0)
        {
            ct.ThrowIfCancellationRequested();
            string current = pending.Pop();

            try
            {
                foreach (string file in Directory.EnumerateFiles(current))
                {
                    try
                    {
                        total += new FileInfo(file).Length;
                    }
                    catch
                    {
                    }
                }

                foreach (string subDir in Directory.EnumerateDirectories(current))
                {
                    try
                    {
                        var info = new DirectoryInfo(subDir);
                        if ((info.Attributes & FileAttributes.ReparsePoint) == 0)
                        {
                            pending.Push(subDir);
                        }
                    }
                    catch
                    {
                    }
                }
            }
            catch
            {
            }
        }

        return total;
    }

    private static string NormalizeDirectoryPath(string path) =>
        Path.TrimEndingDirectorySeparator(Path.GetFullPath(path));
}
