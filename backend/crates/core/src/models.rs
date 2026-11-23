use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TranslationProviderKind {
    MyMemory,
    Azure,
    Google,
}

impl Default for TranslationProviderKind {
    fn default() -> Self {
        Self::MyMemory
    }
}

impl TranslationProviderKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MyMemory => "mymemory",
            Self::Azure => "azure",
            Self::Google => "google",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::MyMemory => "MyMemory",
            Self::Azure => "Azure Translator",
            Self::Google => "Google Cloud Translation",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TtsExecutionMode {
    Auto,
    Cuda,
    Cpu,
}

impl Default for TtsExecutionMode {
    fn default() -> Self {
        Self::Auto
    }
}

impl TtsExecutionMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Cuda => "cuda",
            Self::Cpu => "cpu",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaptureRegion {
    pub id: String,
    pub name: String,
    pub monitor_id: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub dpi_scale: f64,
    pub source_language: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MonitorInfo {
    pub id: String,
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub dpi_scale: f64,
    pub is_primary: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OverlaySettings {
    pub font_size: f64,
    pub opacity: f64,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub locked_click_through: bool,
}

impl Default for OverlaySettings {
    fn default() -> Self {
        Self {
            font_size: 24.0,
            opacity: 0.85,
            x: 260,
            y: 780,
            width: 960,
            height: 150,
            locked_click_through: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderCredentials {
    pub mymemory_email: Option<String>,
    pub azure_api_key: Option<String>,
    pub azure_region: Option<String>,
    pub azure_endpoint: Option<String>,
    pub google_api_key: Option<String>,
}

impl Default for ProviderCredentials {
    fn default() -> Self {
        Self {
            mymemory_email: None,
            azure_api_key: None,
            azure_region: None,
            azure_endpoint: None,
            google_api_key: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppSettings {
    pub monitoring_interval_ms: u64,
    pub active_provider: TranslationProviderKind,
    pub fallback_providers: Vec<TranslationProviderKind>,
    pub credentials: ProviderCredentials,
    pub cloud_consent_granted: bool,
    pub tts_enabled: bool,
    pub tts_volume: f32,
    pub tts_execution_mode: TtsExecutionMode,
    pub overlay: OverlaySettings,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            monitoring_interval_ms: 200, // 5 checks/sec default (within 4-6 Hz target)
            active_provider: TranslationProviderKind::MyMemory,
            fallback_providers: Vec::new(),
            credentials: ProviderCredentials::default(),
            cloud_consent_granted: false,
            tts_enabled: true,
            tts_volume: 1.0,
            tts_execution_mode: TtsExecutionMode::Auto,
            overlay: OverlaySettings::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct GpuTelemetry {
    pub gpu_available: bool,
    pub gpu_name: Option<String>,
    pub driver_version: Option<String>,
    pub vram_total_mb: u64,
    pub vram_used_mb: u64,
    pub vram_free_mb: u64,
    pub gpu_utilization_pct: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkerRuntimeState {
    pub ready: bool,
    pub execution_device: String,
    pub fallback_reason: Option<String>,
    pub restart_count: u32,
    pub last_error: Option<String>,
}

impl Default for WorkerRuntimeState {
    fn default() -> Self {
        Self {
            ready: false,
            execution_device: "initializing".to_string(),
            fallback_reason: None,
            restart_count: 0,
            last_error: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct StageLatenciesMs {
    pub capture_ms: u64,
    pub ocr_ms: u64,
    pub stabilization_ms: u64,
    pub translation_ms: u64,
    pub tts_synthesis_ms: Option<u64>,
    pub first_audio_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OcrRecognitionOutput {
    pub region_id: String,
    pub sequence: u64,
    pub source_language: String,
    pub text: String,
    pub confidence: f32,
    pub inference_device: String,
    pub latency_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubtitleEvent {
    pub region_id: String,
    pub sequence: u64,
    pub source_language: String,
    pub original_text: String,
    pub vietnamese_text: String,
    pub provider: String,
    pub from_cache: bool,
    pub ocr_device: String,
    pub tts_device: String,
    pub latencies: StageLatenciesMs,
    pub timestamp_utc: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: i64,
    pub timestamp_utc: DateTime<Utc>,
    pub region_id: String,
    pub source_language: String,
    pub ocr_text: String,
    pub vietnamese_translation: String,
    pub provider: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppStateSnapshot {
    pub monitoring_active: bool,
    pub regions: Vec<CaptureRegion>,
    pub monitors: Vec<MonitorInfo>,
    pub settings: AppSettings,
    pub gpu: GpuTelemetry,
    pub ocr_worker: WorkerRuntimeState,
    pub tts_worker: WorkerRuntimeState,
    pub current_subtitle: Option<SubtitleEvent>,
    pub provider_status: String,
    pub overlay_capture_excluded: bool,
}
