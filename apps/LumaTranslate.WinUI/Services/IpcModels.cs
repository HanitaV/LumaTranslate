using System.Text.Json.Serialization;

namespace LumaTranslate_WinUI.Services;

public sealed class CaptureRegionDto
{
    [JsonPropertyName("id")]
    public string Id { get; set; } = string.Empty;

    [JsonPropertyName("name")]
    public string Name { get; set; } = string.Empty;

    [JsonPropertyName("monitor_id")]
    public string MonitorId { get; set; } = string.Empty;

    [JsonPropertyName("x")]
    public int X { get; set; }

    [JsonPropertyName("y")]
    public int Y { get; set; }

    [JsonPropertyName("width")]
    public uint Width { get; set; } = 400;

    [JsonPropertyName("height")]
    public uint Height { get; set; } = 120;

    [JsonPropertyName("dpi_scale")]
    public double DpiScale { get; set; } = 1.0;

    [JsonPropertyName("source_language")]
    public string SourceLanguage { get; set; } = "auto";

    [JsonPropertyName("enabled")]
    public bool Enabled { get; set; } = true;
}

public sealed class MonitorInfoDto
{
    [JsonPropertyName("id")]
    public string Id { get; set; } = string.Empty;

    [JsonPropertyName("name")]
    public string Name { get; set; } = string.Empty;

    [JsonPropertyName("x")]
    public int X { get; set; }

    [JsonPropertyName("y")]
    public int Y { get; set; }

    [JsonPropertyName("width")]
    public uint Width { get; set; }

    [JsonPropertyName("height")]
    public uint Height { get; set; }

    [JsonPropertyName("dpi_scale")]
    public double DpiScale { get; set; } = 1.0;

    [JsonPropertyName("is_primary")]
    public bool IsPrimary { get; set; }
}

public sealed class OverlaySettingsDto
{
    [JsonPropertyName("font_size")]
    public double FontSize { get; set; } = 24.0;

    [JsonPropertyName("opacity")]
    public double Opacity { get; set; } = 0.85;

    [JsonPropertyName("x")]
    public int X { get; set; } = 260;

    [JsonPropertyName("y")]
    public int Y { get; set; } = 780;

    [JsonPropertyName("width")]
    public uint Width { get; set; } = 960;

    [JsonPropertyName("height")]
    public uint Height { get; set; } = 150;

    [JsonPropertyName("locked_click_through")]
    public bool LockedClickThrough { get; set; } = true;

    public OverlaySettingsDto Clone() => new()
    {
        FontSize = FontSize,
        Opacity = Opacity,
        X = X,
        Y = Y,
        Width = Width,
        Height = Height,
        LockedClickThrough = LockedClickThrough,
    };

    public bool ContentEquals(OverlaySettingsDto? other)
    {
        if (other is null)
        {
            return false;
        }

        return Math.Abs(FontSize - other.FontSize) < 0.01
            && Math.Abs(Opacity - other.Opacity) < 0.005
            && X == other.X
            && Y == other.Y
            && Width == other.Width
            && Height == other.Height
            && LockedClickThrough == other.LockedClickThrough;
    }
}

public sealed class ProviderCredentialsDto
{
    [JsonPropertyName("mymemory_email")]
    public string? MyMemoryEmail { get; set; }

    [JsonPropertyName("azure_api_key")]
    public string? AzureApiKey { get; set; }

    [JsonPropertyName("azure_region")]
    public string? AzureRegion { get; set; }

    [JsonPropertyName("azure_endpoint")]
    public string? AzureEndpoint { get; set; }

    [JsonPropertyName("google_api_key")]
    public string? GoogleApiKey { get; set; }
}

public sealed class AppSettingsDto
{
    [JsonPropertyName("monitoring_interval_ms")]
    public ulong MonitoringIntervalMs { get; set; } = 200;

    [JsonPropertyName("active_provider")]
    public string ActiveProvider { get; set; } = "mymemory";

    [JsonPropertyName("fallback_providers")]
    public List<string> FallbackProviders { get; set; } = [];

    [JsonPropertyName("credentials")]
    public ProviderCredentialsDto Credentials { get; set; } = new();

    [JsonPropertyName("cloud_consent_granted")]
    public bool CloudConsentGranted { get; set; }

    [JsonPropertyName("tts_enabled")]
    public bool TtsEnabled { get; set; } = true;

    [JsonPropertyName("tts_volume")]
    public float TtsVolume { get; set; } = 1.0f;

    [JsonPropertyName("tts_execution_mode")]
    public string TtsExecutionMode { get; set; } = "auto";

    [JsonPropertyName("overlay")]
    public OverlaySettingsDto Overlay { get; set; } = new();
}

public sealed class GpuTelemetryDto
{
    [JsonPropertyName("gpu_available")]
    public bool GpuAvailable { get; set; }

    [JsonPropertyName("gpu_name")]
    public string? GpuName { get; set; }

    [JsonPropertyName("driver_version")]
    public string? DriverVersion { get; set; }

    [JsonPropertyName("vram_total_mb")]
    public ulong VramTotalMb { get; set; }

    [JsonPropertyName("vram_used_mb")]
    public ulong VramUsedMb { get; set; }

    [JsonPropertyName("vram_free_mb")]
    public ulong VramFreeMb { get; set; }

    [JsonPropertyName("gpu_utilization_pct")]
    public uint? GpuUtilizationPct { get; set; }
}

public sealed class WorkerRuntimeStateDto
{
    [JsonPropertyName("ready")]
    public bool Ready { get; set; }

    [JsonPropertyName("execution_device")]
    public string ExecutionDevice { get; set; } = "initializing";

    [JsonPropertyName("fallback_reason")]
    public string? FallbackReason { get; set; }

    [JsonPropertyName("restart_count")]
    public uint RestartCount { get; set; }

    [JsonPropertyName("last_error")]
    public string? LastError { get; set; }
}

public sealed class StageLatenciesDto
{
    [JsonPropertyName("capture_ms")]
    public ulong CaptureMs { get; set; }

    [JsonPropertyName("ocr_ms")]
    public ulong OcrMs { get; set; }

    [JsonPropertyName("stabilization_ms")]
    public ulong StabilizationMs { get; set; }

    [JsonPropertyName("translation_ms")]
    public ulong TranslationMs { get; set; }

    [JsonPropertyName("tts_synthesis_ms")]
    public ulong? TtsSynthesisMs { get; set; }

    [JsonPropertyName("first_audio_ms")]
    public ulong? FirstAudioMs { get; set; }
}

public sealed class SubtitleEventDto
{
    [JsonPropertyName("region_id")]
    public string RegionId { get; set; } = string.Empty;

    [JsonPropertyName("sequence")]
    public ulong Sequence { get; set; }

    [JsonPropertyName("source_language")]
    public string SourceLanguage { get; set; } = string.Empty;

    [JsonPropertyName("original_text")]
    public string OriginalText { get; set; } = string.Empty;

    [JsonPropertyName("vietnamese_text")]
    public string VietnameseText { get; set; } = string.Empty;

    [JsonPropertyName("provider")]
    public string Provider { get; set; } = string.Empty;

    [JsonPropertyName("from_cache")]
    public bool FromCache { get; set; }

    [JsonPropertyName("ocr_device")]
    public string OcrDevice { get; set; } = string.Empty;

    [JsonPropertyName("tts_device")]
    public string TtsDevice { get; set; } = string.Empty;

    [JsonPropertyName("latencies")]
    public StageLatenciesDto Latencies { get; set; } = new();

    [JsonPropertyName("timestamp_utc")]
    public DateTimeOffset TimestampUtc { get; set; }
}

public sealed class HistoryEntryDto
{
    [JsonPropertyName("id")]
    public long Id { get; set; }

    [JsonPropertyName("timestamp_utc")]
    public DateTimeOffset TimestampUtc { get; set; }

    [JsonPropertyName("region_id")]
    public string RegionId { get; set; } = string.Empty;

    [JsonPropertyName("source_language")]
    public string SourceLanguage { get; set; } = string.Empty;

    [JsonPropertyName("ocr_text")]
    public string OcrText { get; set; } = string.Empty;

    [JsonPropertyName("vietnamese_translation")]
    public string VietnameseTranslation { get; set; } = string.Empty;

    [JsonPropertyName("provider")]
    public string Provider { get; set; } = string.Empty;
}

public sealed class TranslationTestResultDto
{
    [JsonPropertyName("provider")]
    public string Provider { get; set; } = string.Empty;

    [JsonPropertyName("sample_source")]
    public string SampleSource { get; set; } = string.Empty;

    [JsonPropertyName("vietnamese_output")]
    public string VietnameseOutput { get; set; } = string.Empty;

    [JsonPropertyName("latency_ms")]
    public ulong LatencyMs { get; set; }
}

public sealed class AppStateSnapshotDto
{
    [JsonPropertyName("monitoring_active")]
    public bool MonitoringActive { get; set; }

    [JsonPropertyName("regions")]
    public List<CaptureRegionDto> Regions { get; set; } = [];

    [JsonPropertyName("monitors")]
    public List<MonitorInfoDto> Monitors { get; set; } = [];

    [JsonPropertyName("settings")]
    public AppSettingsDto Settings { get; set; } = new();

    [JsonPropertyName("gpu")]
    public GpuTelemetryDto Gpu { get; set; } = new();

    [JsonPropertyName("ocr_worker")]
    public WorkerRuntimeStateDto OcrWorker { get; set; } = new();

    [JsonPropertyName("tts_worker")]
    public WorkerRuntimeStateDto TtsWorker { get; set; } = new();

    [JsonPropertyName("current_subtitle")]
    public SubtitleEventDto? CurrentSubtitle { get; set; }

    [JsonPropertyName("provider_status")]
    public string ProviderStatus { get; set; } = string.Empty;

    [JsonPropertyName("overlay_capture_excluded")]
    public bool OverlayCaptureExcluded { get; set; } = true;
}

public sealed class IpcServerEnvelope
{
    [JsonPropertyName("type")]
    public string Type { get; set; } = string.Empty;

    [JsonPropertyName("request_id")]
    public ulong? RequestId { get; set; }

    [JsonPropertyName("ok")]
    public bool? Ok { get; set; }

    [JsonPropertyName("error")]
    public string? Error { get; set; }

    [JsonPropertyName("state")]
    public AppStateSnapshotDto? State { get; set; }

    [JsonPropertyName("monitors")]
    public List<MonitorInfoDto>? Monitors { get; set; }

    [JsonPropertyName("history")]
    public List<HistoryEntryDto>? History { get; set; }

    [JsonPropertyName("test_translation_result")]
    public TranslationTestResultDto? TestTranslationResult { get; set; }

    [JsonPropertyName("subtitle")]
    public SubtitleEventDto? Subtitle { get; set; }

    [JsonPropertyName("ocr_worker")]
    public WorkerRuntimeStateDto? OcrWorker { get; set; }

    [JsonPropertyName("tts_worker")]
    public WorkerRuntimeStateDto? TtsWorker { get; set; }

    [JsonPropertyName("region_id")]
    public string? RegionId { get; set; }

    [JsonPropertyName("sequence")]
    public ulong? Sequence { get; set; }

    [JsonPropertyName("latencies")]
    public StageLatenciesDto? Latencies { get; set; }

    [JsonPropertyName("code")]
    public string? Code { get; set; }

    [JsonPropertyName("message_vi")]
    public string? MessageVi { get; set; }

    [JsonPropertyName("detail")]
    public string? Detail { get; set; }
}
