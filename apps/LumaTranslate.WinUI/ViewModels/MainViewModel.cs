using System.Collections.ObjectModel;
using System.ComponentModel;
using System.Runtime.CompilerServices;
using LumaTranslate_WinUI.Services;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;

namespace LumaTranslate_WinUI.ViewModels;

public sealed record LanguageOption(string Code, string DisplayVi)
{
    public override string ToString() => DisplayVi;
}

public sealed record ProviderOption(string Id, string DisplayVi)
{
    public override string ToString() => DisplayVi;
}

public sealed record TtsModeOption(string Id, string DisplayVi)
{
    public override string ToString() => DisplayVi;
}

public sealed class RegionItemViewModel : INotifyPropertyChanged
{
    private CaptureRegionDto _dto;

    public RegionItemViewModel(CaptureRegionDto dto)
    {
        _dto = dto;
    }

    public CaptureRegionDto Dto => _dto;
    public string Id => _dto.Id;
    public string Name => _dto.Name;
    public bool Enabled
    {
        get => _dto.Enabled;
        set
        {
            if (_dto.Enabled != value)
            {
                _dto.Enabled = value;
                OnPropertyChanged();
            }
        }
    }

    public string SourceLanguage => _dto.SourceLanguage;
    public IReadOnlyList<LanguageOption> AvailableLanguages => MainViewModel.SupportedLanguages;
    public LanguageOption SelectedLanguageOption =>
        MainViewModel.SupportedLanguages.FirstOrDefault(
            l => string.Equals(l.Code, _dto.SourceLanguage, StringComparison.OrdinalIgnoreCase))
        ?? MainViewModel.SupportedLanguages[0];
    public string LanguageDisplay => MainViewModel.FormatLanguageVi(_dto.SourceLanguage);
    public string GeometrySummary => $"Tọa độ ({_dto.X}, {_dto.Y}) • Kích thước {_dto.Width}×{_dto.Height} px";

    public void UpdateFrom(CaptureRegionDto updated)
    {
        _dto = updated;
        OnPropertyChanged(string.Empty);
    }

    public event PropertyChangedEventHandler? PropertyChanged;
    private void OnPropertyChanged([CallerMemberName] string? name = null) =>
        PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(name));
}

public sealed class HistoryItemViewModel
{
    public HistoryItemViewModel(HistoryEntryDto dto)
    {
        Id = dto.Id;
        TimestampText = dto.TimestampUtc.ToLocalTime().ToString("HH:mm:ss dd/MM/yyyy");
        RegionId = dto.RegionId;
        LanguageDisplay = MainViewModel.FormatLanguageVi(dto.SourceLanguage);
        OcrText = dto.OcrText;
        VietnameseTranslation = dto.VietnameseTranslation;
        Provider = dto.Provider;
    }

    public long Id { get; }
    public string TimestampText { get; }
    public string RegionId { get; }
    public string LanguageDisplay { get; }
    public string OcrText { get; }
    public string VietnameseTranslation { get; }
    public string Provider { get; }
}

public sealed class MainViewModel : INotifyPropertyChanged
{
    public static IReadOnlyList<LanguageOption> SupportedLanguages { get; } =
    [
        new("auto", "Tự động nhận diện (Anh / Nhật / Trung / Hàn)"),
        new("en", "Tiếng Anh (English)"),
        new("ja", "Tiếng Nhật (Japanese)"),
        new("zh", "Tiếng Trung giản thể"),
        new("zh-Hant", "Tiếng Trung phồn thể"),
        new("ko", "Tiếng Hàn (Korean)"),
        new("latin", "Hệ chữ Latinh (Pháp / Đức / TBN / BĐN)"),
    ];

    public static IReadOnlyList<ProviderOption> SupportedProviders { get; } =
    [
        new("mymemory", "MyMemory (Mặc định • Không cần khóa API)"),
        new("azure", "Azure Translator (REST v3.0 • Hỗ trợ gói F0)"),
        new("google", "Google Cloud Translation (REST v2)"),
    ];

    public static IReadOnlyList<TtsModeOption> SupportedTtsModes { get; } =
    [
        new("auto", "Tự động (Ưu tiên CUDA, chuyển CPU nếu thiếu VRAM)"),
        new("cuda", "CUDA (PyTorch GPU)"),
        new("cpu", "CPU (ONNX Runtime)"),
    ];

    private readonly BackendProcessManager _processManager = new();
    private readonly BackendIpcClient _ipcClient = new();
    private DispatcherQueue? _dispatcher;
    private AppSettingsDto _settings = new();
    private OverlaySettingsDto? _lastAppliedOverlaySettings;
    private List<MonitorInfoDto> _monitors = [];

    private bool _isConnected;
    private bool _monitoringActive;
    private bool _togglingMonitoring;
    private string _currentOriginalText = "Chưa có văn bản nhận dạng.";
    private string _currentVietnameseText = "Chọn một vùng màn hình và nhấn \"Bắt đầu dịch\" để dịch phụ đề sang tiếng Việt.";
    private string _latencySummary = "Độ trễ: —";
    private string _ocrStatusText = "Đang khởi tạo...";
    private string _ttsStatusText = "VieNeu-TTS: Đang khởi tạo...";
    private string _gpuSummaryText = "Đang kiểm tra GPU NVIDIA...";
    private string _providerStatusText = "MyMemory (Chưa bắt đầu)";
    private string _monitoringStatusText = "Đã dừng";
    private string? _bannerMessage;
    private bool _bannerIsError;
    private bool _overlayVisible = true;
    private string _defaultNewRegionLanguage = "auto";

    public ObservableCollection<RegionItemViewModel> Regions { get; } = [];
    public ObservableCollection<HistoryItemViewModel> HistoryItems { get; } = [];

    public MainViewModel()
    {
        Regions.CollectionChanged += (_, _) =>
        {
            OnPropertyChanged(nameof(EmptyRegionsVisibility));
            OnPropertyChanged(nameof(RegionsListVisibility));
        };
    }

    public event Action<SubtitleEventDto>? SubtitleChangedForOverlay;
    public event Action<OverlaySettingsDto>? OverlaySettingsChanged;

    public BackendIpcClient Ipc => _ipcClient;
    public IReadOnlyList<MonitorInfoDto> Monitors => _monitors;
    public AppSettingsDto Settings => _settings;

    public bool IsConnected
    {
        get => _isConnected;
        private set => SetField(ref _isConnected, value);
    }

    public bool MonitoringActive
    {
        get => _monitoringActive;
        private set
        {
            if (SetField(ref _monitoringActive, value))
            {
                OnPropertyChanged(nameof(StartStopButtonText));
                MonitoringStatusText = value ? "Đang theo dõi liên tục" : "Đã dừng";
            }
        }
    }

    public string StartStopButtonText => _monitoringActive ? "■ Dừng dịch" : "▶ Bắt đầu dịch";

    public string CurrentOriginalText
    {
        get => _currentOriginalText;
        private set => SetField(ref _currentOriginalText, value);
    }

    public string CurrentVietnameseText
    {
        get => _currentVietnameseText;
        private set => SetField(ref _currentVietnameseText, value);
    }

    public string LatencySummary
    {
        get => _latencySummary;
        private set => SetField(ref _latencySummary, value);
    }

    public string OcrStatusText
    {
        get => _ocrStatusText;
        private set => SetField(ref _ocrStatusText, value);
    }

    public string TtsStatusText
    {
        get => _ttsStatusText;
        private set => SetField(ref _ttsStatusText, value);
    }

    public string GpuSummaryText
    {
        get => _gpuSummaryText;
        private set => SetField(ref _gpuSummaryText, value);
    }

    public string ProviderStatusText
    {
        get => _providerStatusText;
        private set => SetField(ref _providerStatusText, value);
    }

    public string MonitoringStatusText
    {
        get => _monitoringStatusText;
        private set => SetField(ref _monitoringStatusText, value);
    }

    public bool NeedsCloudConsent => !_settings.CloudConsentGranted;
    public Visibility EmptyRegionsVisibility => Regions.Count == 0 ? Visibility.Visible : Visibility.Collapsed;
    public Visibility RegionsListVisibility => Regions.Count > 0 ? Visibility.Visible : Visibility.Collapsed;
    public bool IsOverlayMoveMode => !_settings.Overlay.LockedClickThrough;
    public string MoveOverlayButtonText => IsOverlayMoveMode ? "✓ Khóa vị trí Vietsub" : "✥ Di chuyển Vietsub";
    public bool IsTtsEnabled => _settings.TtsEnabled;

    public bool OverlayVisible
    {
        get => _overlayVisible;
        set => SetField(ref _overlayVisible, value);
    }

    public string DefaultNewRegionLanguage
    {
        get => _defaultNewRegionLanguage;
        set => SetField(ref _defaultNewRegionLanguage, value);
    }

    public string? BannerMessage
    {
        get => _bannerMessage;
        private set
        {
            if (SetField(ref _bannerMessage, value))
            {
                OnPropertyChanged(nameof(HasBanner));
            }
        }
    }

    public bool HasBanner => !string.IsNullOrWhiteSpace(_bannerMessage);

    public bool BannerIsError
    {
        get => _bannerIsError;
        private set => SetField(ref _bannerIsError, value);
    }

    public void Initialize(DispatcherQueue dispatcher)
    {
        if (_dispatcher is not null)
        {
            return;
        }
        _dispatcher = dispatcher;

        _ipcClient.ConnectionStateChanged += connected => EnqueueUi(() =>
        {
            IsConnected = connected;
            if (connected)
            {
                _ = RefreshStateAsync();
            }
            else
            {
                MonitoringStatusText = "Đang kết nối tiến trình nền...";
            }
        });

        _ipcClient.StateSnapshotReceived += snap => EnqueueUi(() => ApplyStateSnapshot(snap));
        _ipcClient.SubtitleUpdated += sub => EnqueueUi(() => ApplySubtitleUpdate(sub));
        _ipcClient.WorkerStatusUpdated += (ocr, tts) => EnqueueUi(() => ApplyWorkerStates(ocr, tts));
        _ipcClient.SpeechLatencyUpdated += (regId, seq, lat) => EnqueueUi(() =>
        {
            LatencySummary = FormatLatencies(lat, cached: false);
        });
        _ipcClient.PipelineErrorReceived += (code, msgVi) => EnqueueUi(() =>
        {
            ShowBanner(msgVi, isError: true);
        });

        bool launched = _processManager.EnsureBackendRunning();
        if (!launched)
        {
            ShowBanner("Không tìm thấy tệp thực thi LumaTranslate.Backend.exe. Hãy chạy scripts\\build.ps1.", isError: true);
        }
        _ipcClient.StartBackgroundConnection();
    }

    public async Task RefreshStateAsync()
    {
        try
        {
            await _ipcClient.GetStateAsync();
        }
        catch (Exception ex)
        {
            ShowBanner($"Không thể tải trạng thái từ tiến trình nền: {ex.Message}", isError: true);
        }
    }

    private void ApplyStateSnapshot(AppStateSnapshotDto snap)
    {
        MonitoringActive = snap.MonitoringActive;
        _monitors = snap.Monitors;
        _settings = snap.Settings;
        OnPropertyChanged(nameof(NeedsCloudConsent));
        OnPropertyChanged(nameof(Settings));
        OnPropertyChanged(nameof(IsOverlayMoveMode));
        OnPropertyChanged(nameof(MoveOverlayButtonText));
        OnPropertyChanged(nameof(IsTtsEnabled));

        // Sync region collection preserving UI selection where possible
        var incomingById = snap.Regions.ToDictionary(r => r.Id);
        for (int i = Regions.Count - 1; i >= 0; i--)
        {
            if (!incomingById.ContainsKey(Regions[i].Id))
            {
                Regions.RemoveAt(i);
            }
        }
        foreach (var dto in snap.Regions)
        {
            var existing = Regions.FirstOrDefault(r => r.Id == dto.Id);
            if (existing is not null)
            {
                existing.UpdateFrom(dto);
            }
            else
            {
                Regions.Add(new RegionItemViewModel(dto));
            }
        }

        ProviderStatusText = string.IsNullOrWhiteSpace(snap.ProviderStatus)
            ? FormatProviderName(_settings.ActiveProvider)
            : snap.ProviderStatus;

        if (snap.Gpu.GpuAvailable)
        {
            GpuSummaryText =
                $"{snap.Gpu.GpuName} • VRAM trống {snap.Gpu.VramFreeMb}/{snap.Gpu.VramTotalMb} MB (Driver {snap.Gpu.DriverVersion})";
        }
        else
        {
            GpuSummaryText = "Không phát hiện GPU NVIDIA (Đang dùng chế độ dự phòng CPU)";
        }

        ApplyWorkerStates(snap.OcrWorker, snap.TtsWorker);

        if (snap.CurrentSubtitle is not null)
        {
            ApplySubtitleUpdate(snap.CurrentSubtitle, notifyOverlay: false);
        }

        if (_lastAppliedOverlaySettings is null || !_settings.Overlay.ContentEquals(_lastAppliedOverlaySettings))
        {
            _lastAppliedOverlaySettings = _settings.Overlay.Clone();
            OverlaySettingsChanged?.Invoke(_settings.Overlay.Clone());
        }
    }

    public void UpdateOverlayBoundsFromUser(int x, int y, uint width, uint height)
    {
        _settings.Overlay.X = x;
        _settings.Overlay.Y = y;
        _settings.Overlay.Width = width;
        _settings.Overlay.Height = height;
        _lastAppliedOverlaySettings = _settings.Overlay.Clone();
    }

    private void ApplyWorkerStates(WorkerRuntimeStateDto ocr, WorkerRuntimeStateDto tts)
    {
        OcrStatusText = FormatWorkerStateVi("PP-OCRv5 Mobile", ocr);
        TtsStatusText = FormatWorkerStateVi("VieNeu-TTS v3 Turbo", tts);
    }

    private static string FormatWorkerStateVi(string modelTitle, WorkerRuntimeStateDto state)
    {
        if (!string.IsNullOrWhiteSpace(state.LastError))
        {
            return $"{modelTitle} (Lỗi: {state.LastError})";
        }
        string devLower = (state.ExecutionDevice ?? "initializing").Trim().ToLowerInvariant();
        if (!state.Ready || devLower == "initializing")
        {
            return $"{modelTitle} (Đang nạp mô hình AI...)";
        }
        string devUpper = devLower.ToUpperInvariant();
        return state.FallbackReason is null
            ? $"{modelTitle} ({devUpper} • Sẵn sàng)"
            : $"{modelTitle} ({devUpper} • Dự phòng: {state.FallbackReason})";
    }

    private void ApplySubtitleUpdate(SubtitleEventDto sub, bool notifyOverlay = true)
    {
        CurrentOriginalText = sub.OriginalText;
        CurrentVietnameseText = sub.VietnameseText;
        ProviderStatusText = sub.FromCache
            ? $"{sub.Provider} (Bộ nhớ đệm)"
            : sub.Provider;
        LatencySummary = FormatLatencies(sub.Latencies, sub.FromCache);

        if (notifyOverlay)
        {
            SubtitleChangedForOverlay?.Invoke(sub);
        }
    }

    private static string FormatLatencies(StageLatenciesDto lat, bool cached)
    {
        string transTag = cached ? "0ms (cache)" : $"{lat.TranslationMs}ms";
        string ttsTag = lat.FirstAudioMs.HasValue
            ? $" • Âm thanh đầu: {lat.FirstAudioMs.Value}ms"
            : string.Empty;
        return $"Chụp: {lat.CaptureMs}ms • OCR: {lat.OcrMs}ms • Ổn định: {lat.StabilizationMs}ms • Dịch: {transTag}{ttsTag}";
    }

    public async Task GrantCloudConsentAsync()
    {
        _settings.CloudConsentGranted = true;
        await SaveSettingsAsync(_settings, "Đã xác nhận quyền riêng tư cho dịch đám mây.");
    }

    public async Task SaveSettingsAsync(AppSettingsDto updated, string? successNotice = null)
    {
        try
        {
            IpcServerEnvelope resp = await _ipcClient.UpdateSettingsAsync(updated);
            if (resp.Ok == false)
            {
                ShowBanner(resp.Error ?? "Không thể lưu cài đặt.", isError: true);
                return;
            }
            if (!string.IsNullOrWhiteSpace(successNotice))
            {
                ShowBanner(successNotice, isError: false);
            }
        }
        catch (Exception ex)
        {
            ShowBanner($"Lỗi lưu cài đặt: {ex.Message}", isError: true);
        }
    }

    public async Task AddOrUpdateRegionAsync(CaptureRegionDto region)
    {
        try
        {
            IpcServerEnvelope resp = await _ipcClient.UpsertRegionAsync(region);
            if (resp.Ok == false)
            {
                ShowBanner(resp.Error ?? "Vùng chọn không hợp lệ.", isError: true);
            }
            else
            {
                ClearBanner();
            }
        }
        catch (Exception ex)
        {
            ShowBanner($"Lỗi cập nhật vùng màn hình: {ex.Message}", isError: true);
        }
    }

    public async Task DeleteRegionAsync(string regionId)
    {
        try
        {
            await _ipcClient.DeleteRegionAsync(regionId);
        }
        catch (Exception ex)
        {
            ShowBanner($"Lỗi xóa vùng: {ex.Message}", isError: true);
        }
    }

    public async Task SetRegionEnabledAsync(string regionId, bool enabled)
    {
        try
        {
            await _ipcClient.SetRegionEnabledAsync(regionId, enabled);
        }
        catch (Exception ex)
        {
            ShowBanner($"Lỗi bật/tắt vùng: {ex.Message}", isError: true);
        }
    }

    public async Task SetRegionLanguageAsync(string regionId, string languageCode)
    {
        RegionItemViewModel? existing = Regions.FirstOrDefault(r => r.Id == regionId);
        if (existing is null || string.Equals(existing.SourceLanguage, languageCode, StringComparison.OrdinalIgnoreCase))
        {
            return;
        }
        CaptureRegionDto src = existing.Dto;
        var updated = new CaptureRegionDto
        {
            Id = src.Id,
            Name = src.Name,
            MonitorId = src.MonitorId,
            X = src.X,
            Y = src.Y,
            Width = src.Width,
            Height = src.Height,
            DpiScale = src.DpiScale,
            SourceLanguage = languageCode,
            Enabled = src.Enabled,
        };
        DefaultNewRegionLanguage = languageCode;
        await AddOrUpdateRegionAsync(updated);
    }

    public async Task SetOverlayMoveModeAsync(bool allowMove)
    {
        _settings.Overlay.LockedClickThrough = !allowMove;
        if (allowMove)
        {
            App.MainWindowInstance?.SetSubtitleOverlayVisibility(true);
        }
        _lastAppliedOverlaySettings = _settings.Overlay.Clone();
        OnPropertyChanged(nameof(IsOverlayMoveMode));
        OnPropertyChanged(nameof(MoveOverlayButtonText));
        OverlaySettingsChanged?.Invoke(_settings.Overlay.Clone());

        string notice = allowMove
            ? "Đã mở khóa khung Vietsub: hãy giữ chuột kéo khung trên màn hình hoặc kéo góc ↘ để đổi kích thước, sau đó nhấn '✓ Khóa vị trí'."
            : "Đã khóa cố định vị trí khung Vietsub (chế độ xuyên chuột).";
        await SaveSettingsAsync(_settings, notice);
    }

    public async Task SetTtsEnabledQuickAsync(bool enabled)
    {
        if (_settings.TtsEnabled == enabled)
        {
            return;
        }
        _settings.TtsEnabled = enabled;
        OnPropertyChanged(nameof(IsTtsEnabled));
        await SaveSettingsAsync(_settings);
    }

    public async Task<(bool Ok, string MessageVi)> TestTtsPlaybackAsync(string? sampleText = null)
    {
        try
        {
            IpcServerEnvelope resp = await _ipcClient.TestTtsAsync(sampleText);
            if (resp.Ok == true)
            {
                return (true, "Đang phát giọng đọc tiếng Việt thử nghiệm qua loa/tai nghe mặc định.");
            }
            return (false, resp.Error ?? "Không thể phát giọng đọc thử nghiệm.");
        }
        catch (Exception ex)
        {
            return (false, $"Lỗi kiểm tra TTS: {ex.Message}");
        }
    }

    public async Task ToggleMonitoringAsync()
    {
        if (_togglingMonitoring)
        {
            return;
        }
        _togglingMonitoring = true;
        try
        {
            ClearBanner();
            if (MonitoringActive)
            {
                await _ipcClient.StopMonitoringAsync();
            }
            else
            {
                IpcServerEnvelope resp = await _ipcClient.StartMonitoringAsync();
                if (resp.Ok == false)
                {
                    ShowBanner(resp.Error ?? "Không thể bắt đầu dịch.", isError: true);
                }
            }
        }
        catch (Exception ex)
        {
            ShowBanner($"Lỗi điều khiển theo dõi: {ex.Message}", isError: true);
        }
        finally
        {
            _togglingMonitoring = false;
        }
    }

    public async Task LoadHistoryAsync(string? searchQuery = null)
    {
        try
        {
            IpcServerEnvelope resp = await _ipcClient.QueryHistoryAsync(searchQuery, 200, 0);
            if (resp.Ok == true && resp.History is not null)
            {
                EnqueueUi(() =>
                {
                    HistoryItems.Clear();
                    foreach (var item in resp.History)
                    {
                        HistoryItems.Add(new HistoryItemViewModel(item));
                    }
                });
            }
        }
        catch (Exception ex)
        {
            ShowBanner($"Lỗi tải lịch sử hội thoại: {ex.Message}", isError: true);
        }
    }

    public async Task ClearHistoryAsync()
    {
        try
        {
            await _ipcClient.ClearHistoryAsync();
            EnqueueUi(() => HistoryItems.Clear());
            ShowBanner("Đã xóa toàn bộ lịch sử hội thoại.", isError: false);
        }
        catch (Exception ex)
        {
            ShowBanner($"Lỗi xóa lịch sử: {ex.Message}", isError: true);
        }
    }

    public async Task ClearCacheAsync()
    {
        try
        {
            await _ipcClient.ClearCacheAsync();
            ShowBanner("Đã xóa bộ nhớ đệm bản dịch.", isError: false);
        }
        catch (Exception ex)
        {
            ShowBanner($"Lỗi xóa bộ nhớ đệm: {ex.Message}", isError: true);
        }
    }

    public async Task<(bool Ok, string MessageVi)> TestProviderConnectionAsync(
        string providerId,
        AppSettingsDto draftSettings)
    {
        try
        {
            IpcServerEnvelope resp = await _ipcClient.TestTranslationAsync(providerId, draftSettings);
            if (resp.Ok == true && resp.TestTranslationResult is not null)
            {
                var r = resp.TestTranslationResult;
                return (true, $"Kết nối {r.Provider} thành công ({r.LatencyMs} ms): \"{r.VietnameseOutput}\"");
            }
            return (false, resp.Error ?? "Kiểm tra kết nối thất bại.");
        }
        catch (Exception ex)
        {
            return (false, $"Lỗi kiểm tra kết nối: {ex.Message}");
        }
    }

    public void ShowBanner(string message, bool isError)
    {
        EnqueueUi(() =>
        {
            BannerIsError = isError;
            BannerMessage = message;
        });
    }

    public void ClearBanner() => EnqueueUi(() => BannerMessage = null);

    public async Task ShutdownAsync()
    {
        await _ipcClient.RequestBackendShutdownAsync();
        await _ipcClient.DisposeAsync();
        await _processManager.ShutdownAsync();
    }

    public static string FormatLanguageVi(string code) =>
        code.ToLowerInvariant() switch
        {
            "ja" or "japan" => "Tiếng Nhật",
            "en" or "english" => "Tiếng Anh",
            "zh" or "zh-cn" or "ch" => "Tiếng Trung (Giản thể)",
            "zh-hant" or "zh-tw" or "chinese_cht" => "Tiếng Trung (Phồn thể)",
            "ko" or "korean" => "Tiếng Hàn",
            "latin" => "Hệ chữ Latinh",
            _ => "Tự động",
        };

    public static string FormatProviderName(string id) =>
        id.ToLowerInvariant() switch
        {
            "azure" => "Azure Translator",
            "google" => "Google Cloud Translation",
            _ => "MyMemory",
        };

    private void EnqueueUi(DispatcherQueueHandler callback)
    {
        if (_dispatcher is null || _dispatcher.HasThreadAccess)
        {
            callback();
        }
        else
        {
            _dispatcher.TryEnqueue(callback);
        }
    }

    public event PropertyChangedEventHandler? PropertyChanged;

    private bool SetField<T>(ref T field, T value, [CallerMemberName] string? propertyName = null)
    {
        if (EqualityComparer<T>.Default.Equals(field, value))
        {
            return false;
        }
        field = value;
        OnPropertyChanged(propertyName);
        return true;
    }

    private void OnPropertyChanged([CallerMemberName] string? propertyName = null) =>
        PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(propertyName));
}
