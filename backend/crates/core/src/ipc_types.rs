use crate::models::{
    AppSettings, AppStateSnapshot, CaptureRegion, HistoryEntry, MonitorInfo, StageLatenciesMs,
    SubtitleEvent, TranslationProviderKind, WorkerRuntimeState,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum IpcRequest {
    GetState {
        request_id: u64,
    },
    ListMonitors {
        request_id: u64,
    },
    UpsertRegion {
        request_id: u64,
        region: CaptureRegion,
    },
    DeleteRegion {
        request_id: u64,
        region_id: String,
    },
    SetRegionEnabled {
        request_id: u64,
        region_id: String,
        enabled: bool,
    },
    StartMonitoring {
        request_id: u64,
    },
    StopMonitoring {
        request_id: u64,
    },
    UpdateSettings {
        request_id: u64,
        settings: AppSettings,
    },
    RegisterOverlayWindow {
        request_id: u64,
        hwnd: u64,
        capture_excluded: bool,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    },
    TestTranslation {
        request_id: u64,
        provider: Option<TranslationProviderKind>,
        settings_override: Option<AppSettings>,
    },
    QueryHistory {
        request_id: u64,
        search_query: Option<String>,
        limit: usize,
        offset: usize,
    },
    ClearHistory {
        request_id: u64,
    },
    ClearCache {
        request_id: u64,
    },
    Shutdown {
        request_id: u64,
    },
}

impl IpcRequest {
    pub fn request_id(&self) -> u64 {
        match self {
            Self::GetState { request_id }
            | Self::ListMonitors { request_id }
            | Self::UpsertRegion { request_id, .. }
            | Self::DeleteRegion { request_id, .. }
            | Self::SetRegionEnabled { request_id, .. }
            | Self::StartMonitoring { request_id }
            | Self::StopMonitoring { request_id }
            | Self::UpdateSettings { request_id, .. }
            | Self::RegisterOverlayWindow { request_id, .. }
            | Self::TestTranslation { request_id, .. }
            | Self::QueryHistory { request_id, .. }
            | Self::ClearHistory { request_id }
            | Self::ClearCache { request_id }
            | Self::Shutdown { request_id } => *request_id,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum IpcServerMessage {
    Response {
        request_id: u64,
        ok: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        state: Option<AppStateSnapshot>,
        #[serde(skip_serializing_if = "Option::is_none")]
        monitors: Option<Vec<MonitorInfo>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        history: Option<Vec<HistoryEntry>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        test_translation_result: Option<TranslationTestResult>,
    },
    StateSnapshot {
        state: AppStateSnapshot,
    },
    SubtitleUpdated {
        subtitle: SubtitleEvent,
    },
    WorkerStatus {
        ocr_worker: WorkerRuntimeState,
        tts_worker: WorkerRuntimeState,
    },
    SpeechLatencyUpdated {
        region_id: String,
        sequence: u64,
        latencies: StageLatenciesMs,
    },
    PipelineError {
        code: String,
        message_vi: String,
        detail: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TranslationTestResult {
    pub provider: String,
    pub sample_source: String,
    pub vietnamese_output: String,
    pub latency_ms: u64,
}
