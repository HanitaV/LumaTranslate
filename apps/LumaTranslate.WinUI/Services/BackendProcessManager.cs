using System.Diagnostics;

namespace LumaTranslate_WinUI.Services;

public sealed class BackendProcessManager : IDisposable
{
    private Process? _backendProcess;
    private bool _disposed;

    public string PipeName { get; }

    public BackendProcessManager(string pipeName = @"\\.\pipe\LumaTranslate.IPC")
    {
        PipeName = pipeName;
    }

    public bool EnsureBackendRunning()
    {
        if (_backendProcess is { HasExited: false })
        {
            return true;
        }

        string shortPipeName = PipeName.Replace(@"\\.\pipe\", string.Empty, StringComparison.OrdinalIgnoreCase);
        if (Directory.GetFiles(@"\\.\pipe\", shortPipeName).Length > 0)
        {
            return true;
        }

        ResolvedEnvironmentPaths env = EnvironmentBootstrapService.InspectEnvironment();
        if (string.IsNullOrWhiteSpace(env.BackendExePath))
        {
            return false;
        }

        var psi = new ProcessStartInfo
        {
            FileName = env.BackendExePath,
            UseShellExecute = false,
            CreateNoWindow = true,
            RedirectStandardError = false,
            RedirectStandardOutput = false,
        };
        psi.ArgumentList.Add("--pipe-name");
        psi.ArgumentList.Add(PipeName);
        if (!string.IsNullOrWhiteSpace(env.WorkersDir))
        {
            psi.ArgumentList.Add("--workers-dir");
            psi.ArgumentList.Add(env.WorkersDir);
        }
        if (!string.IsNullOrWhiteSpace(env.ModelsDir))
        {
            psi.ArgumentList.Add("--models-dir");
            psi.ArgumentList.Add(env.ModelsDir);
        }
        if (!string.IsNullOrWhiteSpace(env.OcrPythonExe))
        {
            psi.ArgumentList.Add("--ocr-python");
            psi.ArgumentList.Add(env.OcrPythonExe);
        }
        if (!string.IsNullOrWhiteSpace(env.TtsPythonExe))
        {
            psi.ArgumentList.Add("--tts-python");
            psi.ArgumentList.Add(env.TtsPythonExe);
        }

        _backendProcess = Process.Start(psi);
        return _backendProcess is not null;
    }

    public static string? DiscoverBackendExecutable()
    {
        string? envOverride = Environment.GetEnvironmentVariable("LUMA_BACKEND_EXE");
        if (!string.IsNullOrWhiteSpace(envOverride) && File.Exists(envOverride))
        {
            return envOverride;
        }

        string baseDir = AppContext.BaseDirectory;
        string[] exeNames = ["LumaTranslate.Backend.exe", "luma_translate_backend.exe"];
        foreach (string name in exeNames)
        {
            string colocated = Path.Combine(baseDir, name);
            if (File.Exists(colocated))
            {
                return colocated;
            }
            string packagedSubdir = Path.Combine(baseDir, "backend", name);
            if (File.Exists(packagedSubdir))
            {
                return packagedSubdir;
            }
        }

        DirectoryInfo? cursor = new(baseDir);
        while (cursor is not null)
        {
            foreach (string profile in new[] { "release", "debug" })
            {
                foreach (string name in exeNames)
                {
                    string candidate = Path.Combine(
                        cursor.FullName, "backend", "target", profile, name);
                    if (File.Exists(candidate))
                    {
                        return candidate;
                    }
                }
            }
            cursor = cursor.Parent;
        }

        return null;
    }

    public static string? DiscoverWorkersDirectory(string? backendExePath)
    {
        string? envDir = Environment.GetEnvironmentVariable("LUMA_WORKERS_DIR");
        if (!string.IsNullOrWhiteSpace(envDir) && Directory.Exists(envDir))
        {
            return envDir;
        }

        string appBaseWorkers = Path.Combine(AppContext.BaseDirectory, "workers");
        if (File.Exists(Path.Combine(appBaseWorkers, "ocr", "worker.py")))
        {
            return appBaseWorkers;
        }

        if (!string.IsNullOrWhiteSpace(backendExePath))
        {
            DirectoryInfo? cursor = new FileInfo(backendExePath).Directory;
            while (cursor is not null)
            {
                string candidate = Path.Combine(cursor.FullName, "workers");
                if (File.Exists(Path.Combine(candidate, "ocr", "worker.py")))
                {
                    return candidate;
                }
                cursor = cursor.Parent;
            }
        }

        return null;
    }

    public void Dispose()
    {
        if (_disposed)
        {
            return;
        }
        _disposed = true;

        try
        {
            if (_backendProcess is { HasExited: false })
            {
                if (!_backendProcess.WaitForExit(1500))
                {
                    _backendProcess.Kill(entireProcessTree: true);
                }
            }
        }
        catch
        {
        }
        finally
        {
            _backendProcess?.Dispose();
            _backendProcess = null;
        }
    }
}
