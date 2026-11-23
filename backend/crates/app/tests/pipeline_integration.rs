//! Comprehensive integration tests covering all required real-time pipeline scenarios:
//! - English, Japanese, Chinese, and Korean subtitle streams
//! - Gradually appearing typewriter text
//! - Repeated subtitles (cache hit without re-querying cloud provider)
//! - Multiple concurrent regions
//! - Rapid subtitle replacement & out-of-order cloud response stale protection
//! - Translation timeout, offline error, and cloud quota exhaustion
//! - OCR worker crash & automatic recovery
//! - Immediate stop during active TTS playback

use async_trait::async_trait;
use luma_app::{CoordinatorIpcAdapter, PipelineCoordinator};
use luma_capture::{CaptureError, CroppedFrame, ScreenCapturer};
use luma_core::{
    AppSettings, CaptureRegion, GpuTelemetry, IpcRequest, IpcServerMessage, MonitorInfo,
    OcrRecognitionOutput, TranslationProviderKind, TtsExecutionMode, WorkerRuntimeState,
};
use luma_ipc::IpcCommandHandler;
use luma_ocr_client::{OcrBackend, OcrClientError};
use luma_storage::LumaStore;
use luma_translation::{CloudTranslationManager, TranslationError, TranslationProvider};
use luma_tts_client::{SpeechSynthesizer, TtsClientError};
use parking_lot::Mutex;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

struct ScriptedCapturer {
    frames: Mutex<HashMap<String, VecDeque<u8>>>,
    last_seed: Mutex<HashMap<String, u8>>,
}

impl ScriptedCapturer {
    fn new() -> Self {
        Self {
            frames: Mutex::new(HashMap::new()),
            last_seed: Mutex::new(HashMap::new()),
        }
    }

    fn push_frame_seed(&self, region_id: &str, seed: u8) {
        self.frames
            .lock()
            .entry(region_id.to_string())
            .or_default()
            .push_back(seed);
    }
}

impl ScreenCapturer for ScriptedCapturer {
    fn list_monitors(&self) -> Result<Vec<MonitorInfo>, CaptureError> {
        Ok(vec![MonitorInfo {
            id: "mon-primary".into(),
            name: "Primary 1920x1080".into(),
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
            dpi_scale: 1.0,
            is_primary: true,
        }])
    }

    fn capture_region(&self, region: &CaptureRegion) -> Result<CroppedFrame, CaptureError> {
        let seed = {
            let mut map = self.frames.lock();
            if let Some(q) = map.get_mut(&region.id) {
                if let Some(next) = q.pop_front() {
                    self.last_seed.lock().insert(region.id.clone(), next);
                    next
                } else {
                    *self.last_seed.lock().get(&region.id).unwrap_or(&0)
                }
            } else {
                0
            }
        };

        let w = 64u32;
        let h = 32u32;
        // Encode seed in first byte for OCR lookup, and fill remaining bytes with high-contrast
        // distinct luma (`seed.wrapping_mul(47)`) so FrameDiffDetector sees real visual changes.
        let fill = seed.wrapping_mul(47);
        let mut bgra = vec![fill; (w * h * 4) as usize];
        bgra[0] = seed;
        Ok(CroppedFrame {
            width: w,
            height: h,
            stride: w * 4,
            bgra8: bgra,
            capture_latency_ms: 2,
        })
    }

    fn release_unused_monitors(&self, _active_monitor_ids: &[String]) {}
    fn shutdown(&self) {}
}

struct ScriptedOcrBackend {
    seed_to_text: Mutex<HashMap<u8, (String, String)>>,
    fail_next: AtomicBool,
    restarts: AtomicUsize,
}

impl ScriptedOcrBackend {
    fn new() -> Self {
        Self {
            seed_to_text: Mutex::new(HashMap::new()),
            fail_next: AtomicBool::new(false),
            restarts: AtomicUsize::new(0),
        }
    }

    fn map_seed(&self, seed: u8, text: &str, lang: &str) {
        self.seed_to_text
            .lock()
            .insert(seed, (text.to_string(), lang.to_string()));
    }
}

#[async_trait]
impl OcrBackend for ScriptedOcrBackend {
    async fn recognize_bgra8(
        &self,
        region_id: &str,
        sequence: u64,
        source_language: &str,
        _width: u32,
        _height: u32,
        _stride: u32,
        bgra_bytes: &[u8],
    ) -> Result<OcrRecognitionOutput, OcrClientError> {
        if self.fail_next.swap(false, Ordering::SeqCst) {
            self.restarts.fetch_add(1, Ordering::SeqCst);
            return Err(OcrClientError::Ipc("Simulated worker crash".into()));
        }
        let seed = bgra_bytes.first().copied().unwrap_or(0);
        let (text, lang) = self
            .seed_to_text
            .lock()
            .get(&seed)
            .cloned()
            .unwrap_or_else(|| (String::new(), source_language.to_string()));

        Ok(OcrRecognitionOutput {
            region_id: region_id.to_string(),
            sequence,
            source_language: lang,
            text,
            confidence: 0.98,
            inference_device: "cuda".into(),
            latency_ms: 15,
        })
    }

    fn runtime_state(&self) -> WorkerRuntimeState {
        WorkerRuntimeState {
            ready: true,
            execution_device: "cuda".into(),
            fallback_reason: None,
            restart_count: self.restarts.load(Ordering::SeqCst) as u32,
            last_error: None,
        }
    }

    fn gpu_telemetry(&self) -> GpuTelemetry {
        GpuTelemetry {
            gpu_available: true,
            gpu_name: Some("RTX 3060 Test".into()),
            driver_version: Some("621.01".into()),
            vram_total_mb: 6144,
            vram_used_mb: 800,
            vram_free_mb: 5344,
            gpu_utilization_pct: Some(12),
        }
    }

    async fn shutdown(&self) {}
}

struct ControlledCloudProvider {
    delays_ms: Mutex<HashMap<String, u64>>,
    errors: Mutex<HashMap<String, TranslationError>>,
    translations: Mutex<HashMap<String, String>>,
    call_count: AtomicUsize,
}

impl ControlledCloudProvider {
    fn new() -> Self {
        let mut map = HashMap::new();
        map.insert(
            "Hello! Welcome back.".to_string(),
            "Xin chào! Chào mừng quay trở lại.".to_string(),
        );
        map.insert(
            "また会えて嬉しい。".to_string(),
            "Rất vui khi được gặp lại bạn.".to_string(),
        );
        map.insert(
            "很高兴再次见到你。".to_string(),
            "Rất vui được gặp lại bạn lần nữa.".to_string(),
        );
        map.insert(
            "다시 만나서 반가워요.".to_string(),
            "Rất vui được gặp lại bạn.".to_string(),
        );
        map.insert("Old slow subtitle".to_string(), "Phụ đề cũ bị trễ".to_string());
        map.insert("New fast subtitle".to_string(), "Phụ đề mới nhanh hơn".to_string());
        Self {
            delays_ms: Mutex::new(HashMap::new()),
            errors: Mutex::new(HashMap::new()),
            translations: Mutex::new(map),
            call_count: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl TranslationProvider for ControlledCloudProvider {
    async fn translate(
        &self,
        source_text: &str,
        _source_language: Option<&str>,
    ) -> Result<String, TranslationError> {
        self.call_count.fetch_add(1, Ordering::SeqCst);
        let delay = self.delays_ms.lock().get(source_text).copied().unwrap_or(5);
        tokio::time::sleep(Duration::from_millis(delay)).await;
        if let Some(err) = self.errors.lock().get(source_text).cloned() {
            return Err(err);
        }
        Ok(self
            .translations
            .lock()
            .get(source_text)
            .cloned()
            .unwrap_or_else(|| format!("VI({source_text})")))
    }
}

struct RecordingTtsMock {
    spoken: Mutex<Vec<String>>,
    stop_calls: AtomicUsize,
    next_id: AtomicU64,
    active_playing: AtomicBool,
}

impl RecordingTtsMock {
    fn new() -> Self {
        Self {
            spoken: Mutex::new(Vec::new()),
            stop_calls: AtomicUsize::new(0),
            next_id: AtomicU64::new(1),
            active_playing: AtomicBool::new(false),
        }
    }
}

#[async_trait]
impl SpeechSynthesizer for RecordingTtsMock {
    async fn speak_latest(&self, vietnamese_text: &str) -> Result<u64, TtsClientError> {
        self.active_playing.store(true, Ordering::SeqCst);
        self.spoken.lock().push(vietnamese_text.to_string());
        Ok(self.next_id.fetch_add(1, Ordering::SeqCst))
    }

    async fn stop_all(&self) {
        self.active_playing.store(false, Ordering::SeqCst);
        self.stop_calls.fetch_add(1, Ordering::SeqCst);
    }

    async fn set_execution_mode(&self, _mode: TtsExecutionMode) -> Result<(), TtsClientError> {
        Ok(())
    }

    fn set_volume(&self, _volume: f32) {}

    fn runtime_state(&self) -> WorkerRuntimeState {
        WorkerRuntimeState {
            ready: true,
            execution_device: "cuda".into(),
            fallback_reason: None,
            restart_count: 0,
            last_error: None,
        }
    }

    async fn shutdown(&self) {
        self.stop_all().await;
    }
}

#[tokio::test]
async fn multilingual_regions_typewriter_cache_reuse_and_stop_playback_work() {
    let db_path = std::env::temp_dir().join(format!("luma_int_1_{}.db", std::process::id()));
    let _ = std::fs::remove_file(&db_path);
    let store = Arc::new(LumaStore::open_at_path(db_path.clone(), 128).unwrap());

    let capturer = Arc::new(ScriptedCapturer::new());
    let ocr = Arc::new(ScriptedOcrBackend::new());
    let cloud = Arc::new(ControlledCloudProvider::new());
    let translator = Arc::new(CloudTranslationManager::new(Arc::clone(&store)));
    translator.register_custom_provider(TranslationProviderKind::MyMemory, cloud.clone());
    let tts = Arc::new(RecordingTtsMock::new());

    // Map pixel seeds to EN, JA (typewriter + full), ZH, KO, and blank
    ocr.map_seed(0, "", "auto");
    ocr.map_seed(10, "また", "ja");
    ocr.map_seed(11, "また会えて嬉しい。", "ja");
    ocr.map_seed(20, "Hello! Welcome back.", "en");
    ocr.map_seed(30, "很高兴再次见到你。", "zh");
    ocr.map_seed(40, "다시 만나서 반가워요.", "ko");

    let (events_tx, mut events_rx) = broadcast::channel(128);
    let shutdown = CancellationToken::new();
    let coord = PipelineCoordinator::new(
        Arc::clone(&store),
        capturer.clone(),
        ocr.clone(),
        translator,
        tts.clone(),
        events_tx,
        shutdown.clone(),
        None,
    );
    let adapter = CoordinatorIpcAdapter(Arc::clone(&coord));

    // Configure consent + 100ms interval
    let settings = AppSettings {
        monitoring_interval_ms: 100,
        cloud_consent_granted: true,
        tts_enabled: true,
        ..Default::default()
    };
    adapter
        .handle_request(IpcRequest::UpdateSettings {
            request_id: 1,
            settings,
        })
        .await;

    // Add 2 regions simultaneously (Region 1: Japanese/Chinese, Region 2: English/Korean)
    for (id, lang, y) in [("r-cjk", "ja", 700), ("r-enko", "en", 860)] {
        adapter
            .handle_request(IpcRequest::UpsertRegion {
                request_id: 2,
                region: CaptureRegion {
                    id: id.into(),
                    name: id.into(),
                    monitor_id: "mon-primary".into(),
                    x: 100,
                    y,
                    width: 600,
                    height: 100,
                    dpi_scale: 1.0,
                    source_language: lang.into(),
                    enabled: true,
                },
            })
            .await;
    }

    // Queue frames:
    // r-cjk: partial "また" (1 frame) -> full JA (2 frames) -> blank (1 frame) -> repeated JA (2 frames) -> ZH (2 frames)
    for seed in [10u8, 11, 11, 0, 11, 11, 30, 30] {
        capturer.push_frame_seed("r-cjk", seed);
    }
    // r-enko: EN (2 frames) -> KO (2 frames)
    for seed in [20u8, 20, 40, 40] {
        capturer.push_frame_seed("r-enko", seed);
    }

    adapter
        .handle_request(IpcRequest::StartMonitoring { request_id: 3 })
        .await;

    let mut received_vi = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(4);
    while received_vi.len() < 5 && tokio::time::Instant::now() < deadline {
        if let Ok(Ok(msg)) =
            tokio::time::timeout(Duration::from_millis(200), events_rx.recv()).await
        {
            if let IpcServerMessage::SubtitleUpdated { subtitle } = msg {
                received_vi.push((subtitle.original_text, subtitle.vietnamese_text, subtitle.from_cache));
            }
        }
    }

    // Verify all 4 languages + repeated JA line were emitted, partial "また" was NOT emitted!
    assert_eq!(received_vi.len(), 5, "Got subtitles: {received_vi:?}");
    assert!(!received_vi.iter().any(|(orig, _, _)| orig == "また"));
    // Repeated "また会えて嬉しい。" hit SQLite/memory cache on its 2nd appearance!
    assert!(received_vi.iter().any(|(orig, _, cached)| orig == "また会えて嬉しい。" && *cached));
    // Cloud provider was called only 4 times (EN, JA once, ZH, KO), NOT 5!
    assert_eq!(cloud.call_count.load(Ordering::SeqCst), 4);

    // Stop during active TTS playback must immediately stop audio & cancel pending TTS
    assert!(tts.active_playing.load(Ordering::SeqCst));
    adapter
        .handle_request(IpcRequest::StopMonitoring { request_id: 4 })
        .await;
    assert!(!tts.active_playing.load(Ordering::SeqCst));
    assert!(tts.stop_calls.load(Ordering::SeqCst) >= 1);

    // History persisted all 5 dialogue occurrences
    let hist = store.query_history(None, 50, 0).unwrap();
    assert_eq!(hist.len(), 5);

    coord.shutdown_all().await;
    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn prevents_stale_out_of_order_translations_and_handles_errors_and_worker_crash() {
    let db_path = std::env::temp_dir().join(format!("luma_int_2_{}.db", std::process::id()));
    let _ = std::fs::remove_file(&db_path);
    let store = Arc::new(LumaStore::open_at_path(db_path.clone(), 64).unwrap());

    let capturer = Arc::new(ScriptedCapturer::new());
    let ocr = Arc::new(ScriptedOcrBackend::new());
    let cloud = Arc::new(ControlledCloudProvider::new());

    // Make "Old slow subtitle" take 500ms, while "New fast subtitle" takes 10ms!
    cloud
        .delays_ms
        .lock()
        .insert("Old slow subtitle".into(), 550);
    cloud
        .delays_ms
        .lock()
        .insert("New fast subtitle".into(), 10);
    cloud.errors.lock().insert(
        "Quota trigger".into(),
        TranslationError::QuotaExceeded("MyMemory".into(), "Daily limit".into()),
    );
    cloud.errors.lock().insert(
        "Offline trigger".into(),
        TranslationError::Network("MyMemory".into(), "DNS unreachable".into()),
    );
    cloud.errors.lock().insert(
        "Timeout trigger".into(),
        TranslationError::Timeout("MyMemory".into()),
    );

    ocr.map_seed(1, "Old slow subtitle", "en");
    ocr.map_seed(2, "New fast subtitle", "en");
    ocr.map_seed(3, "Quota trigger", "en");
    ocr.map_seed(4, "Offline trigger", "en");
    ocr.map_seed(5, "Timeout trigger", "en");

    let translator = Arc::new(CloudTranslationManager::new(Arc::clone(&store)));
    translator.register_custom_provider(TranslationProviderKind::MyMemory, cloud);
    let tts = Arc::new(RecordingTtsMock::new());

    let (events_tx, mut events_rx) = broadcast::channel(128);
    let shutdown = CancellationToken::new();
    let coord = PipelineCoordinator::new(
        Arc::clone(&store),
        capturer.clone(),
        ocr.clone(),
        translator,
        tts,
        events_tx,
        shutdown,
        None,
    );
    let adapter = CoordinatorIpcAdapter(Arc::clone(&coord));

    adapter
        .handle_request(IpcRequest::UpdateSettings {
            request_id: 1,
            settings: AppSettings {
                monitoring_interval_ms: 100,
                cloud_consent_granted: true,
                tts_enabled: true,
                ..Default::default()
            },
        })
        .await;
    adapter
        .handle_request(IpcRequest::UpsertRegion {
            request_id: 2,
            region: CaptureRegion {
                id: "r1".into(),
                name: "R1".into(),
                monitor_id: "mon-primary".into(),
                x: 50,
                y: 50,
                width: 400,
                height: 80,
                dpi_scale: 1.0,
                source_language: "en".into(),
                enabled: true,
            },
        })
        .await;

    // Inject simulated worker crash on very first frame, then rapid subtitle replacement
    // ("Old slow subtitle" takes 550ms; 200ms later "New fast subtitle" stabilizes & finishes in 10ms;
    // when "Old slow subtitle" returns 340ms after that, stale sequence protection discards it!),
    // followed by quota, offline, and timeout errors.
    ocr.fail_next.store(true, Ordering::SeqCst);
    for seed in [1u8, 1, 1, 2, 2, 2, 2, 2, 2, 3, 3, 4, 4, 5, 5] {
        capturer.push_frame_seed("r1", seed);
    }

    adapter
        .handle_request(IpcRequest::StartMonitoring { request_id: 3 })
        .await;

    let mut subtitles = Vec::new();
    let mut errors = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    while tokio::time::Instant::now() < deadline {
        if let Ok(Ok(msg)) =
            tokio::time::timeout(Duration::from_millis(150), events_rx.recv()).await
        {
            match msg {
                IpcServerMessage::SubtitleUpdated { subtitle } => {
                    subtitles.push(subtitle.vietnamese_text);
                }
                IpcServerMessage::PipelineError { code, .. } => {
                    errors.push(code);
                }
                _ => {}
            }
        }
        if errors.len() >= 4 {
            break;
        }
    }

    // Old slow subtitle MUST NEVER overwrite New fast subtitle!
    assert_eq!(subtitles, vec!["Phụ đề mới nhanh hơn".to_string()]);
    assert!(errors.contains(&"OCR_ERROR".to_string()));
    assert!(errors.iter().filter(|c| *c == "TRANSLATION_ERROR").count() >= 3);

    coord.shutdown_all().await;
    let _ = std::fs::remove_file(&db_path);
}
