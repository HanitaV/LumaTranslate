using System.Diagnostics;
using System.IO.Compression;
using System.Net.Http;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;

namespace LumaTranslate_WinUI.Services;

public sealed record DetectedGpuInfo(
    bool HasNvidiaGpu,
    bool SupportsCuda126,
    string GpuName,
    int VramTotalMb,
    int VramFreeMb,
    string DriverVersion);

public sealed record ResolvedEnvironmentPaths(
    string? BackendExePath,
    string? WorkersDir,
    string? OcrPythonExe,
    string? TtsPythonExe,
    string ModelsDir,
    string ManagedRuntimesDir,
    bool IsReady,
    string MissingReasonVi);

public sealed record SetupProgressUpdate(
    double Percent,
    string StepTitleVi,
    string? LogLine = null,
    bool IsError = false);

/// <summary>
/// Built-in self-bootstrapper making <c>LumaTranslate.exe</c> act as both the application
/// launcher and the first-run environment installer (Python 3.12 via standalone <c>uv</c>,
/// isolated CUDA/CPU OCR &amp; VieNeu-TTS virtual environments, and local Apache-2.0 AI models).
/// </summary>
public static class EnvironmentBootstrapService
{
    private const string UvWindowsZipUrl =
        "https://github.com/astral-sh/uv/releases/latest/download/uv-x86_64-pc-windows-msvc.zip";

    private static readonly HttpClient SharedHttp = new()
    {
        Timeout = TimeSpan.FromMinutes(10),
    };

    public static string AppLocalDataRoot =>
        Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "LumaTranslate");

    public static string ManagedRuntimesRoot => Path.Combine(AppLocalDataRoot, "runtimes");
    public static string ManagedModelsRoot => Path.Combine(AppLocalDataRoot, "models");
    public static string ManagedToolsRoot => Path.Combine(AppLocalDataRoot, "tools");

    public static ResolvedEnvironmentPaths InspectEnvironment()
    {
        string? backendExe = BackendProcessManager.DiscoverBackendExecutable();
        string? workersDir = BackendProcessManager.DiscoverWorkersDirectory(backendExe);

        string? ocrPy = ResolveExistingWorkerPython("ocr", workersDir);
        string? ttsPy = ResolveExistingWorkerPython("tts", workersDir);

        var missing = new List<string>();
        if (string.IsNullOrWhiteSpace(backendExe))
        {
            missing.Add("Thiếu tệp tiến trình nền LumaTranslate.Backend.exe");
        }
        if (string.IsNullOrWhiteSpace(workersDir))
        {
            missing.Add("Thiếu thư mục mã nguồn worker (workers\\ocr, workers\\tts)");
        }
        if (string.IsNullOrWhiteSpace(ocrPy))
        {
            missing.Add("Chưa cài đặt môi trường Python 3.12 cho PaddleOCR PP-OCRv5");
        }
        if (string.IsNullOrWhiteSpace(ttsPy))
        {
            missing.Add("Chưa cài đặt môi trường Python 3.12 cho VieNeu-TTS v3 Turbo");
        }

        bool isReady = missing.Count == 0;
        string summaryVi = isReady
            ? "Môi trường thực thi đã sẵn sàng."
            : string.Join(" • ", missing);

        return new ResolvedEnvironmentPaths(
            backendExe,
            workersDir,
            ocrPy,
            ttsPy,
            ManagedModelsRoot,
            ManagedRuntimesRoot,
            isReady,
            summaryVi);
    }

    public static string? ResolveExistingWorkerPython(string workerName, string? workersDir)
    {
        string? envOverride = Environment.GetEnvironmentVariable(
            workerName.Equals("ocr", StringComparison.OrdinalIgnoreCase)
                ? "LUMA_OCR_PYTHON"
                : "LUMA_TTS_PYTHON");
        if (!string.IsNullOrWhiteSpace(envOverride) && File.Exists(envOverride))
        {
            return envOverride;
        }

        // 1. Check colocated / repo workers\<name>\.venv\Scripts\python.exe with expected package
        if (!string.IsNullOrWhiteSpace(workersDir))
        {
            string localVenvPy = Path.Combine(workersDir, workerName, ".venv", "Scripts", "python.exe");
            if (IsVenvProvisioned(localVenvPy, workerName))
            {
                return localVenvPy;
            }
        }

        // 2. Check %LOCALAPPDATA%\LumaTranslate\runtimes\<name>\.venv\Scripts\python.exe
        string managedVenvPy = Path.Combine(ManagedRuntimesRoot, workerName, ".venv", "Scripts", "python.exe");
        if (IsVenvProvisioned(managedVenvPy, workerName))
        {
            return managedVenvPy;
        }

        return null;
    }

    private static bool IsVenvProvisioned(string pythonExePath, string workerName)
    {
        if (!File.Exists(pythonExePath))
        {
            return false;
        }

        DirectoryInfo? scriptsDir = new FileInfo(pythonExePath).Directory;
        DirectoryInfo? venvRoot = scriptsDir?.Parent;
        if (venvRoot is null)
        {
            return false;
        }

        string marker = Path.Combine(venvRoot.FullName, ".luma-ready");
        if (File.Exists(marker))
        {
            return true;
        }

        // Also accept pre-existing developer virtualenvs containing site-packages\paddleocr or vieneu
        string sitePackages = Path.Combine(venvRoot.FullName, "Lib", "site-packages");
        string requiredPkgDir = workerName.Equals("ocr", StringComparison.OrdinalIgnoreCase)
            ? Path.Combine(sitePackages, "paddleocr")
            : Path.Combine(sitePackages, "vieneu");

        return Directory.Exists(requiredPkgDir);
    }

    public static async Task<DetectedGpuInfo> DetectGpuAsync(CancellationToken ct = default)
    {
        try
        {
            var psi = new ProcessStartInfo
            {
                FileName = "nvidia-smi",
                Arguments = "--query-gpu=name,memory.total,memory.free,driver_version --format=csv,noheader,nounits",
                UseShellExecute = false,
                CreateNoWindow = true,
                RedirectStandardOutput = true,
                RedirectStandardError = true,
            };

            using var proc = Process.Start(psi);
            if (proc is null)
            {
                return new DetectedGpuInfo(false, false, "Không phát hiện GPU NVIDIA", 0, 0, "N/A");
            }

            string stdout = await proc.StandardOutput.ReadToEndAsync(ct).ConfigureAwait(false);
            await proc.WaitForExitAsync(ct).ConfigureAwait(false);
            if (proc.ExitCode != 0 || string.IsNullOrWhiteSpace(stdout))
            {
                return new DetectedGpuInfo(false, false, "Không phát hiện GPU NVIDIA", 0, 0, "N/A");
            }

            string firstLine = stdout.Split(['\r', '\n'], StringSplitOptions.RemoveEmptyEntries)[0];
            string[] parts = firstLine.Split(',');
            if (parts.Length < 4)
            {
                return new DetectedGpuInfo(false, false, firstLine.Trim(), 0, 0, "N/A");
            }

            string name = parts[0].Trim();
            _ = int.TryParse(parts[1].Trim(), out int totalMb);
            _ = int.TryParse(parts[2].Trim(), out int freeMb);
            string driver = parts[3].Trim();

            string majorPart = driver.Split('.')[0];
            bool cuda126Ok = int.TryParse(majorPart, out int major) && major >= 550;

            return new DetectedGpuInfo(true, cuda126Ok, name, totalMb, freeMb, driver);
        }
        catch
        {
            return new DetectedGpuInfo(false, false, "Không phát hiện GPU NVIDIA (Chế độ CPU)", 0, 0, "N/A");
        }
    }

    public static async Task ProvisionEnvironmentAsync(
        bool downloadModels,
        bool forceReinstall,
        IProgress<SetupProgressUpdate> progress,
        CancellationToken ct)
    {
        Directory.CreateDirectory(AppLocalDataRoot);
        Directory.CreateDirectory(ManagedRuntimesRoot);
        Directory.CreateDirectory(ManagedModelsRoot);
        Directory.CreateDirectory(ManagedToolsRoot);

        ResolvedEnvironmentPaths initial = InspectEnvironment();
        if (string.IsNullOrWhiteSpace(initial.WorkersDir))
        {
            throw new DirectoryNotFoundException(
                "Không tìm thấy thư mục workers\\ocr và workers\\tts đi kèm ứng dụng.");
        }

        progress.Report(new SetupProgressUpdate(5, "Đang kiểm tra phần cứng GPU NVIDIA & trình điều khiển..."));
        DetectedGpuInfo gpu = await DetectGpuAsync(ct).ConfigureAwait(false);
        progress.Report(new SetupProgressUpdate(
            8,
            gpu.SupportsCuda126
                ? $"Đã phát hiện {gpu.GpuName} ({gpu.VramTotalMb} MB VRAM, Driver {gpu.DriverVersion}) — Bật cài đặt CUDA"
                : $"Chế độ thực thi: Dự phòng CPU ({gpu.GpuName})",
            $"[GPU] {gpu.GpuName} | VRAM={gpu.VramFreeMb}/{gpu.VramTotalMb} MB | Driver={gpu.DriverVersion} | CUDA12.6={gpu.SupportsCuda126}"));

        // Step 1: Ensure standalone `uv.exe` & Python 3.12
        progress.Report(new SetupProgressUpdate(12, "Đang chuẩn bị trình quản lý môi trường Python 3.12 (uv)..."));
        string uvExe = await EnsureUvExecutableAsync(progress, ct).ConfigureAwait(false);

        string pythonInstallDir = Path.Combine(ManagedRuntimesRoot, "python");
        Directory.CreateDirectory(pythonInstallDir);
        var uvEnv = new Dictionary<string, string>
        {
            ["UV_PYTHON_INSTALL_DIR"] = pythonInstallDir,
            ["PYTHONIOENCODING"] = "utf-8",
            ["HF_HUB_DISABLE_SYMLINKS_WARNING"] = "1",
            ["PADDLE_PDX_DISABLE_MODEL_SOURCE_CHECK"] = "True",
        };

        await RunProcessStreamingAsync(
            uvExe,
            ["python", "install", "3.12"],
            uvEnv,
            progress,
            15,
            "Đang tải trình thông dịch CPython 3.12 x64 cô lập...",
            ct).ConfigureAwait(false);

        // Step 2: Provision OCR venv
        string? existingOcrPy = forceReinstall ? null : ResolveExistingWorkerPython("ocr", initial.WorkersDir);
        string ocrPy;
        if (!string.IsNullOrWhiteSpace(existingOcrPy))
        {
            ocrPy = existingOcrPy;
            progress.Report(new SetupProgressUpdate(
                42,
                "Môi trường PaddleOCR PP-OCRv5 đã tồn tại — bỏ qua cài đặt lại.",
                $"[OCR] Sử dụng môi trường sẵn có: {ocrPy}"));
        }
        else
        {
            string ocrVenvDir = Path.Combine(ManagedRuntimesRoot, "ocr", ".venv");
            await RunProcessStreamingAsync(
                uvExe,
                ["venv", ocrVenvDir, "--python", "3.12", "--clear"],
                uvEnv,
                progress,
                22,
                "Đang khởi tạo môi trường ảo OCR (Python 3.12)...",
                ct).ConfigureAwait(false);

            ocrPy = Path.Combine(ocrVenvDir, "Scripts", "python.exe");
            if (gpu.SupportsCuda126)
            {
                await RunProcessStreamingAsync(
                    uvExe,
                    [
                        "pip", "install", "--python", ocrPy,
                        "paddlepaddle-gpu==3.3.1",
                        "--index-url", "https://www.paddlepaddle.org.cn/packages/stable/cu126/",
                        "--extra-index-url", "https://pypi.org/simple",
                        "--index-strategy", "unsafe-best-match",
                    ],
                    uvEnv,
                    progress,
                    30,
                    "Đang cài đặt PaddlePaddle GPU 3.3.1 (CUDA 12.6 + cuDNN)...",
                    ct).ConfigureAwait(false);
            }
            else
            {
                await RunProcessStreamingAsync(
                    uvExe,
                    ["pip", "install", "--python", ocrPy, "paddlepaddle==3.3.1"],
                    uvEnv,
                    progress,
                    30,
                    "Đang cài đặt PaddlePaddle CPU 3.3.1...",
                    ct).ConfigureAwait(false);
            }

            await RunProcessStreamingAsync(
                uvExe,
                [
                    "pip", "install", "--python", ocrPy,
                    "paddleocr==3.6.0", "paddlex==3.6.1", "opencv-contrib-python==4.10.0.84",
                    "numpy==2.3.5", "pillow==13.0.0", "nvidia-ml-py==13.615.71",
                ],
                uvEnv,
                progress,
                42,
                "Đang cài đặt PaddleOCR PP-OCRv5 Mobile...",
                ct).ConfigureAwait(false);

            await File.WriteAllTextAsync(
                Path.Combine(ocrVenvDir, ".luma-ready"),
                DateTimeOffset.UtcNow.ToString("O"),
                ct).ConfigureAwait(false);
        }

        // Step 3: Provision TTS venv
        string? existingTtsPy = forceReinstall ? null : ResolveExistingWorkerPython("tts", initial.WorkersDir);
        string ttsPy;
        if (!string.IsNullOrWhiteSpace(existingTtsPy))
        {
            ttsPy = existingTtsPy;
            progress.Report(new SetupProgressUpdate(
                75,
                "Môi trường VieNeu-TTS v3 Turbo đã tồn tại — bỏ qua cài đặt lại.",
                $"[TTS] Sử dụng môi trường sẵn có: {ttsPy}"));
        }
        else
        {
            string ttsVenvDir = Path.Combine(ManagedRuntimesRoot, "tts", ".venv");
            await RunProcessStreamingAsync(
                uvExe,
                ["venv", ttsVenvDir, "--python", "3.12", "--clear"],
                uvEnv,
                progress,
                50,
                "Đang khởi tạo môi trường ảo VieNeu-TTS (Python 3.12)...",
                ct).ConfigureAwait(false);

            ttsPy = Path.Combine(ttsVenvDir, "Scripts", "python.exe");
            if (gpu.SupportsCuda126)
            {
                await RunProcessStreamingAsync(
                    uvExe,
                    [
                        "pip", "install", "--python", ttsPy,
                        "torch==2.8.0", "torchaudio==2.8.0",
                        "--index-url", "https://download.pytorch.org/whl/cu128",
                        "--extra-index-url", "https://pypi.org/simple",
                        "--index-strategy", "unsafe-best-match",
                    ],
                    uvEnv,
                    progress,
                    60,
                    "Đang cài đặt PyTorch 2.8.0 (CUDA 12.8) cho VieNeu-TTS...",
                    ct).ConfigureAwait(false);
            }

            await RunProcessStreamingAsync(
                uvExe,
                [
                    "pip", "install", "--python", ttsPy,
                    "transformers==4.57.6", "vieneu==3.8.5", "onnxruntime>=1.20.0", "nvidia-ml-py>=12.560.30",
                ],
                uvEnv,
                progress,
                72,
                "Đang cài đặt VieNeu-TTS v3 Turbo & ONNX Runtime...",
                ct).ConfigureAwait(false);

            await File.WriteAllTextAsync(
                Path.Combine(ttsVenvDir, ".luma-ready"),
                DateTimeOffset.UtcNow.ToString("O"),
                ct).ConfigureAwait(false);
        }

        // Step 4: Download & verify local OCR + VieNeu-TTS models directly through worker engines
        if (downloadModels)
        {
            string paddleModelsDir = Path.Combine(ManagedModelsRoot, "paddleocr");
            string ttsModelsDir = Path.Combine(ManagedModelsRoot, "tts");
            Directory.CreateDirectory(paddleModelsDir);
            Directory.CreateDirectory(ttsModelsDir);

            string ocrWarmupCode =
                $"import sys, json; sys.path.insert(0, r'{initial.WorkersDir}'); " +
                $"from ocr.engine import PaddleOcrEngine; " +
                $"e = PaddleOcrEngine(preferred_device='cuda', models_dir=r'{paddleModelsDir}'); " +
                $"info = e.initialize(); " +
                $"_ = e._get_or_create_pipeline('korean_PP-OCRv5_mobile_rec'); " +
                $"e.shutdown(); print('OCR_VERIFIED:', json.dumps(info, ensure_ascii=False))";

            await RunProcessStreamingAsync(
                ocrPy,
                ["-u", "-c", ocrWarmupCode],
                uvEnv,
                progress,
                84,
                "Đang tải mô hình PP-OCRv5 Mobile & kiểm chứng thực thi CUDA OCR...",
                ct).ConfigureAwait(false);

            string ttsWarmupCode =
                $"import sys, json; sys.path.insert(0, r'{initial.WorkersDir}'); " +
                $"from tts.engine import VieNeuTtsEngine; " +
                $"e = VieNeuTtsEngine(execution_mode='auto', models_dir=r'{ttsModelsDir}'); " +
                $"info = e.initialize(); e.shutdown(); " +
                $"print('TTS_VERIFIED:', json.dumps(info, ensure_ascii=False))";

            await RunProcessStreamingAsync(
                ttsPy,
                ["-u", "-c", ttsWarmupCode],
                uvEnv,
                progress,
                94,
                "Đang tải mô hình VieNeu-TTS v3 Turbo & kiểm chứng tổng hợp tiếng Việt...",
                ct).ConfigureAwait(false);

            await WriteModelManifestAsync(ManagedModelsRoot, ct).ConfigureAwait(false);
        }

        UninstallService.RegisterWindowsUninstaller();

        progress.Report(new SetupProgressUpdate(
            100,
            "Hoàn tất cài đặt môi trường! Đang khởi động LumaTranslate...",
            "[OK] Tất cả thành phần AI cục bộ đã sẵn sàng."));
    }

    private static async Task<string> EnsureUvExecutableAsync(
        IProgress<SetupProgressUpdate> progress,
        CancellationToken ct)
    {
        // Check managed uv.exe first
        string managedUv = Path.Combine(ManagedToolsRoot, "uv", "uv.exe");
        if (File.Exists(managedUv))
        {
            return managedUv;
        }

        // Check system PATH
        string? pathEnv = Environment.GetEnvironmentVariable("PATH");
        if (!string.IsNullOrWhiteSpace(pathEnv))
        {
            foreach (string dir in pathEnv.Split(Path.PathSeparator, StringSplitOptions.RemoveEmptyEntries))
            {
                try
                {
                    string candidate = Path.Combine(dir.Trim(), "uv.exe");
                    if (File.Exists(candidate))
                    {
                        return candidate;
                    }
                }
                catch
                {
                }
            }
        }

        // Download standalone uv-x86_64-pc-windows-msvc.zip directly without requiring winget or admin rights
        progress.Report(new SetupProgressUpdate(
            13,
            "Đang tải trình quản lý gói độc lập uv.exe từ GitHub Releases...",
            $"[Download] {UvWindowsZipUrl}"));

        string uvDir = Path.Combine(ManagedToolsRoot, "uv");
        Directory.CreateDirectory(uvDir);
        string zipPath = Path.Combine(uvDir, "uv-win-x64.zip");

        using (HttpResponseMessage resp = await SharedHttp.GetAsync(
                   UvWindowsZipUrl, HttpCompletionOption.ResponseHeadersRead, ct).ConfigureAwait(false))
        {
            resp.EnsureSuccessStatusCode();
            await using FileStream fs = File.Create(zipPath);
            await resp.Content.CopyToAsync(fs, ct).ConfigureAwait(false);
        }

        ZipFile.ExtractToDirectory(zipPath, uvDir, overwriteFiles: true);
        File.Delete(zipPath);

        string? extracted = Directory.EnumerateFiles(uvDir, "uv.exe", SearchOption.AllDirectories)
            .FirstOrDefault();
        if (extracted is null)
        {
            throw new FileNotFoundException("Không tìm thấy uv.exe sau khi giải nén gói cài đặt.");
        }
        return extracted;
    }

    private static async Task RunProcessStreamingAsync(
        string fileName,
        IReadOnlyList<string> args,
        IReadOnlyDictionary<string, string> envVars,
        IProgress<SetupProgressUpdate> progress,
        double stepPercent,
        string stepTitleVi,
        CancellationToken ct)
    {
        progress.Report(new SetupProgressUpdate(stepPercent, stepTitleVi, $"> {Path.GetFileName(fileName)} {string.Join(' ', args)}"));

        var psi = new ProcessStartInfo
        {
            FileName = fileName,
            UseShellExecute = false,
            CreateNoWindow = true,
            RedirectStandardOutput = true,
            RedirectStandardError = true,
            StandardOutputEncoding = Encoding.UTF8,
            StandardErrorEncoding = Encoding.UTF8,
        };
        foreach (string a in args)
        {
            psi.ArgumentList.Add(a);
        }
        foreach (var kv in envVars)
        {
            psi.Environment[kv.Key] = kv.Value;
        }

        using var proc = new Process { StartInfo = psi, EnableRaisingEvents = true };
        proc.OutputDataReceived += (_, e) =>
        {
            if (!string.IsNullOrWhiteSpace(e.Data))
            {
                progress.Report(new SetupProgressUpdate(stepPercent, stepTitleVi, e.Data));
            }
        };
        proc.ErrorDataReceived += (_, e) =>
        {
            if (!string.IsNullOrWhiteSpace(e.Data))
            {
                progress.Report(new SetupProgressUpdate(stepPercent, stepTitleVi, e.Data));
            }
        };

        if (!proc.Start())
        {
            throw new InvalidOperationException($"Không thể khởi chạy tiến trình {fileName}");
        }

        proc.BeginOutputReadLine();
        proc.BeginErrorReadLine();

        try
        {
            await proc.WaitForExitAsync(ct).ConfigureAwait(false);
        }
        catch (OperationCanceledException)
        {
            try
            {
                if (!proc.HasExited)
                {
                    proc.Kill(entireProcessTree: true);
                }
            }
            catch
            {
            }
            throw;
        }

        if (proc.ExitCode != 0)
        {
            throw new InvalidOperationException(
                $"Bước '{stepTitleVi}' thất bại với mã thoát {proc.ExitCode}. Xem chi tiết trong nhật ký bên dưới.");
        }
    }

    private static async Task WriteModelManifestAsync(string modelsRoot, CancellationToken ct)
    {
        var verifiedFiles = new List<object>();
        foreach (string file in Directory.EnumerateFiles(modelsRoot, "*.*", SearchOption.AllDirectories))
        {
            string ext = Path.GetExtension(file).ToLowerInvariant();
            if (ext is not (".pdiparams" or ".safetensors" or ".onnx" or ".json"))
            {
                continue;
            }
            var info = new FileInfo(file);
            if (info.Length <= 1024)
            {
                continue;
            }

            await using FileStream stream = File.OpenRead(file);
            byte[] hash = await SHA256.HashDataAsync(stream, ct).ConfigureAwait(false);
            verifiedFiles.Add(new
            {
                relative_path = Path.GetRelativePath(modelsRoot, file),
                size_bytes = info.Length,
                sha256 = Convert.ToHexStringLower(hash),
            });
            if (verifiedFiles.Count >= 25)
            {
                break;
            }
        }

        var manifest = new
        {
            generated_at_utc = DateTimeOffset.UtcNow.ToString("O"),
            models_directory = modelsRoot,
            assets = new object[]
            {
                new
                {
                    name = "PaddleOCR PP-OCRv5 Mobile (det + rec + korean_rec)",
                    version = "PP-OCRv5 (PaddleOCR 3.6.0 / PaddlePaddle 3.3.1)",
                    source_url = "https://huggingface.co/PaddlePaddle/PP-OCRv5_mobile_rec",
                    license = "Apache-2.0",
                },
                new
                {
                    name = "VieNeu-TTS v3 Turbo + MOSS-Audio-Tokenizer-Nano",
                    version = "v3-Turbo (vieneu 3.8.5)",
                    source_url = "https://huggingface.co/pnnbao-ump/VieNeu-TTS-v3-Turbo",
                    license = "Apache-2.0",
                },
            },
            verified_files = verifiedFiles,
        };

        string json = JsonSerializer.Serialize(
            manifest,
            new JsonSerializerOptions { WriteIndented = true });
        await File.WriteAllTextAsync(
            Path.Combine(modelsRoot, "model-manifest.json"),
            json,
            Encoding.UTF8,
            ct).ConfigureAwait(false);
    }
}
