//! Real-time pipeline coordinator and IPC command handler for LumaTranslate.Backend.

use async_trait::async_trait;
use chrono::Utc;
use luma_audio::AudioPlayer;
use luma_capture::{CaptureError, ScreenCapturer};
use luma_core::{
    AppSettings, AppStateSnapshot, CaptureRegion, IpcRequest, IpcServerMessage, MonitorInfo,
    OcrStabilizer, StabilizerConfig, StageLatenciesMs, SubtitleEvent, TranslationTestResult,
};
use luma_ipc::IpcCommandHandler;
use luma_ocr_client::OcrBackend;
use luma_regions::{validate_and_clamp_region, FrameDiffDetector, RectPx};
use luma_storage::LumaStore;
use luma_translation::{CloudTranslationManager, TranslationError};
use luma_tts_client::{SpeechSynthesizer, SpeechTimingReport};
use parking_lot::{Mutex, RwLock};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, mpsc, Semaphore};
use tokio_util::sync::CancellationToken;

const MAX_CONCURRENT_TRANSLATIONS: usize = 4;

#[derive(Debug, Clone, Default)]
struct OverlayWindowInfo {
    hwnd: u64,
    capture_excluded: bool,
    bounds: Option<RectPx>,
}

pub struct PipelineCoordinator {
    store: Arc<LumaStore>,
    capturer: Arc<dyn ScreenCapturer>,
    ocr: Arc<dyn OcrBackend>,
    translator: Arc<CloudTranslationManager>,
    tts: Arc<dyn SpeechSynthesizer>,
    events_tx: broadcast::Sender<IpcServerMessage>,
    root_shutdown: CancellationToken,

    settings: RwLock<AppSettings>,
    regions: RwLock<Vec<CaptureRegion>>,
    current_subtitle: RwLock<Option<SubtitleEvent>>,
    provider_status: RwLock<String>,
    overlay_info: RwLock<OverlayWindowInfo>,

    monitoring_active: AtomicBool,
    monitoring_cancel: Mutex<Option<CancellationToken>>,

    stabilizer: Mutex<OcrStabilizer>,
    frame_diff: Mutex<FrameDiffDetector>,
    region_ocr_sequence: Mutex<HashMap<String, u64>>,
    latest_stabilized_seq: Mutex<HashMap<String, u64>>,
    latest_displayed_seq: Mutex<HashMap<String, u64>>,
    ocr_inflight_regions: Mutex<HashSet<String>>,
    translation_semaphore: Arc<Semaphore>,
    utterance_to_subtitle: Mutex<HashMap<u64, (String, u64, StageLatenciesMs)>>,
}

impl PipelineCoordinator {
    pub fn new(
        store: Arc<LumaStore>,
        capturer: Arc<dyn ScreenCapturer>,
        ocr: Arc<dyn OcrBackend>,
        translator: Arc<CloudTranslationManager>,
        tts: Arc<dyn SpeechSynthesizer>,
        events_tx: broadcast::Sender<IpcServerMessage>,
        root_shutdown: CancellationToken,
        mut timing_rx: Option<mpsc::Receiver<SpeechTimingReport>>,
    ) -> Arc<Self> {
        let loaded_settings = store.load_settings().ok().flatten().unwrap_or_default();
        let loaded_regions = store.load_regions().unwrap_or_default();
        tts.set_volume(loaded_settings.tts_volume);

        let initial_provider_status = format!(
            "Sẵn sàng ({})",
            loaded_settings.active_provider.display_name()
        );

        let coord = Arc::new(Self {
            store,
            capturer,
            ocr,
            translator,
            tts,
            events_tx,
            root_shutdown,
            settings: RwLock::new(loaded_settings),
            regions: RwLock::new(loaded_regions),
            current_subtitle: RwLock::new(None),
            provider_status: RwLock::new(initial_provider_status),
            overlay_info: RwLock::new(OverlayWindowInfo {
                hwnd: 0,
                capture_excluded: true,
                bounds: None,
            }),
            monitoring_active: AtomicBool::new(false),
            monitoring_cancel: Mutex::new(None),
            stabilizer: Mutex::new(OcrStabilizer::new(StabilizerConfig::default())),
            frame_diff: Mutex::new(FrameDiffDetector::default()),
            region_ocr_sequence: Mutex::new(HashMap::new()),
            latest_stabilized_seq: Mutex::new(HashMap::new()),
            latest_displayed_seq: Mutex::new(HashMap::new()),
            ocr_inflight_regions: Mutex::new(HashSet::new()),
            translation_semaphore: Arc::new(Semaphore::new(MAX_CONCURRENT_TRANSLATIONS)),
            utterance_to_subtitle: Mutex::new(HashMap::new()),
        });

        if let Some(mut rx) = timing_rx.take() {
            let weak_coord = Arc::downgrade(&coord);
            tokio::spawn(async move {
                while let Some(report) = rx.recv().await {
                    let Some(c) = weak_coord.upgrade() else {
                        break;
                    };
                    let mapped = c
                        .utterance_to_subtitle
                        .lock()
                        .remove(&report.utterance_id);
                    if let Some((region_id, sequence, mut latencies)) = mapped {
                        latencies.first_audio_ms = Some(report.first_audio_ms);
                        latencies.tts_synthesis_ms = Some(report.total_synthesis_ms);
                        {
                            let mut sub_guard = c.current_subtitle.write();
                            if let Some(ref mut cur) = *sub_guard {
                                if cur.region_id == region_id && cur.sequence == sequence {
                                    cur.latencies = latencies.clone();
                                    cur.tts_device = report.inference_device.clone();
                                }
                            }
                        }
                        let _ = c.events_tx.send(IpcServerMessage::SpeechLatencyUpdated {
                            region_id,
                            sequence,
                            latencies,
                        });
                    }
                }
            });
        }

        coord
    }

    pub fn snapshot(&self) -> AppStateSnapshot {
        let monitors = self.capturer.list_monitors().unwrap_or_default();
        AppStateSnapshot {
            monitoring_active: self.monitoring_active.load(Ordering::Acquire),
            regions: self.regions.read().clone(),
            monitors,
            settings: self.settings.read().clone(),
            gpu: self.ocr.gpu_telemetry(),
            ocr_worker: self.ocr.runtime_state(),
            tts_worker: self.tts.runtime_state(),
            current_subtitle: self.current_subtitle.read().clone(),
            provider_status: self.provider_status.read().clone(),
            overlay_capture_excluded: self.overlay_info.read().capture_excluded,
        }
    }

    pub fn broadcast_snapshot(&self) {
        let _ = self.events_tx.send(IpcServerMessage::StateSnapshot {
            state: self.snapshot(),
        });
    }

    pub async fn start_monitoring(self: &Arc<Self>) -> Result<(), String> {
        {
            let settings = self.settings.read();
            if !settings.cloud_consent_granted {
                return Err(
                    "Vui lòng xác nhận đồng ý gửi văn bản OCR lên dịch vụ dịch đám mây trước khi bắt đầu."
                        .to_string(),
                );
            }
        }
        let has_enabled = self.regions.read().iter().any(|r| r.enabled);
        if !has_enabled {
            return Err("Chưa có vùng màn hình nào được bật để theo dõi.".to_string());
        }

        if self
            .monitoring_active
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Ok(());
        }

        self.frame_diff.lock().reset();
        self.stabilizer.lock().reset_all();
        self.ocr_inflight_regions.lock().clear();

        let run_token = self.root_shutdown.child_token();
        *self.monitoring_cancel.lock() = Some(run_token.clone());

        let coord = Arc::clone(self);
        tokio::spawn(async move {
            coord.monitoring_loop(run_token).await;
        });

        self.broadcast_snapshot();
        Ok(())
    }

    pub async fn stop_monitoring(&self) {
        self.monitoring_active.store(false, Ordering::Release);
        if let Some(tok) = self.monitoring_cancel.lock().take() {
            tok.cancel();
        }
        self.capturer.release_unused_monitors(&[]);
        self.frame_diff.lock().reset();
        self.stabilizer.lock().reset_all();
        self.ocr_inflight_regions.lock().clear();
        self.tts.stop_all().await;
        self.broadcast_snapshot();
    }

    async fn monitoring_loop(self: Arc<Self>, cancel: CancellationToken) {
        loop {
            if cancel.is_cancelled() || !self.monitoring_active.load(Ordering::Acquire) {
                break;
            }

            let interval_ms = self
                .settings
                .read()
                .monitoring_interval_ms
                .clamp(100, 2000);

            let enabled_regions: Vec<CaptureRegion> = self
                .regions
                .read()
                .iter()
                .filter(|r| r.enabled)
                .cloned()
                .collect();

            let active_monitors: Vec<String> = enabled_regions
                .iter()
                .map(|r| r.monitor_id.clone())
                .collect();
            self.capturer.release_unused_monitors(&active_monitors);

            for region in enabled_regions {
                if cancel.is_cancelled() {
                    break;
                }

                // Never queue unbounded OCR requests per region: skip if previous frame still running.
                if self.ocr_inflight_regions.lock().contains(&region.id) {
                    continue;
                }

                {
                    let ov = self.overlay_info.read();
                    if !ov.capture_excluded && ov.hwnd != 0 {
                        if let Some(ov_bounds) = ov.bounds {
                            let reg_rect = RectPx {
                                x: region.x,
                                y: region.y,
                                width: region.width,
                                height: region.height,
                            };
                            if ov_bounds.intersects(reg_rect) {
                                tracing::debug!(
                                    region = %region.id,
                                    hwnd = ov.hwnd,
                                    "Overlay overlaps capture region while WGC exclusion is unsupported; stabilizer feedback guard active"
                                );
                            }
                        }
                    }
                }
                let capturer = Arc::clone(&self.capturer);
                let reg_for_cap = region.clone();
                let cap_res =
                    tokio::task::spawn_blocking(move || capturer.capture_region(&reg_for_cap))
                        .await;

                let frame = match cap_res {
                    Ok(Ok(f)) => f,
                    Ok(Err(CaptureError::FramePending(_))) => continue,
                    Ok(Err(err)) => {
                        tracing::debug!(region = %region.id, "Capture error: {err}");
                        continue;
                    }
                    Err(_) => continue,
                };

                let pending_confirm = self.stabilizer.lock().has_pending_candidate(&region.id);
                let pixel_changed = self.frame_diff.lock().has_changed(
                    &region.id,
                    frame.width,
                    frame.height,
                    &frame.bgra8,
                );

                if !pixel_changed && !pending_confirm {
                    continue;
                }

                self.ocr_inflight_regions.lock().insert(region.id.clone());
                let raw_seq = {
                    let mut seq_map = self.region_ocr_sequence.lock();
                    let entry = seq_map.entry(region.id.clone()).or_insert(0);
                    *entry += 1;
                    *entry
                };

                let coord_ocr = Arc::clone(&self);
                let cancel_ocr = cancel.clone();
                tokio::spawn(async move {
                    coord_ocr
                        .process_region_frame(region, raw_seq, frame, cancel_ocr)
                        .await;
                });
            }

            tokio::select! {
                _ = cancel.cancelled() => break,
                _ = tokio::time::sleep(Duration::from_millis(interval_ms)) => {}
            }
        }
    }

    async fn process_region_frame(
        self: Arc<Self>,
        region: CaptureRegion,
        raw_seq: u64,
        frame: luma_capture::CroppedFrame,
        cancel: CancellationToken,
    ) {
        let ocr_res = self
            .ocr
            .recognize_bgra8(
                &region.id,
                raw_seq,
                &region.source_language,
                frame.width,
                frame.height,
                frame.stride,
                &frame.bgra8,
            )
            .await;

        self.ocr_inflight_regions.lock().remove(&region.id);

        if cancel.is_cancelled() {
            return;
        }

        let ocr_out = match ocr_res {
            Ok(o) => o,
            Err(err) => {
                // Invalidate cached frame luma so the next tick retries OCR after worker recovery.
                self.frame_diff.lock().remove_region(&region.id);
                let _ = self.events_tx.send(IpcServerMessage::PipelineError {
                    code: "OCR_ERROR".into(),
                    message_vi: format!("Lỗi nhận dạng OCR cục bộ: {err}"),
                    detail: Some(err.to_string()),
                });
                return;
            }
        };

        let stabilized = {
            let mut stab = self.stabilizer.lock();
            stab.observe(&region.id, &ocr_out.text, ocr_out.confidence)
        };

        let Some(stab_sub) = stabilized else {
            return;
        };

        {
            let mut latest_stab = self.latest_stabilized_seq.lock();
            let cur = latest_stab.entry(region.id.clone()).or_insert(0);
            if stab_sub.sequence < *cur {
                return;
            }
            *cur = stab_sub.sequence;
        }

        let permit = match Arc::clone(&self.translation_semaphore).try_acquire_owned() {
            Ok(p) => p,
            Err(_) => {
                // Bounded translation concurrency: wait briefly or drop if superseded
                match tokio::time::timeout(
                    Duration::from_secs(2),
                    Arc::clone(&self.translation_semaphore).acquire_owned(),
                )
                .await
                {
                    Ok(Ok(p)) => p,
                    _ => return,
                }
            }
        };

        // Re-check staleness before calling cloud translation
        if cancel.is_cancelled() {
            drop(permit);
            return;
        }
        if self
            .latest_stabilized_seq
            .lock()
            .get(&region.id)
            .copied()
            .unwrap_or(0)
            > stab_sub.sequence
        {
            drop(permit);
            return;
        }

        let settings_snapshot = self.settings.read().clone();
        let trans_res = self
            .translator
            .translate_with_cache(
                &stab_sub.text,
                Some(&region.source_language),
                &settings_snapshot,
                &cancel,
            )
            .await;
        drop(permit);

        if cancel.is_cancelled() {
            return;
        }

        let trans_ok = match trans_res {
            Ok(ok) => {
                *self.provider_status.write() = format!("Hoạt động ({})", ok.provider_used);
                ok
            }
            Err(TranslationError::Cancelled) => return,
            Err(err) => {
                let msg_vi = err.user_message_vi();
                *self.provider_status.write() = format!("Lỗi: {msg_vi}");
                let _ = self.events_tx.send(IpcServerMessage::PipelineError {
                    code: "TRANSLATION_ERROR".into(),
                    message_vi: msg_vi,
                    detail: Some(err.to_string()),
                });
                return;
            }
        };

        // Strict out-of-order / stale sequence protection:
        // NEVER allow an older sequence translation to overwrite a newer subtitle!
        {
            let latest_stab_now = self
                .latest_stabilized_seq
                .lock()
                .get(&region.id)
                .copied()
                .unwrap_or(0);
            if stab_sub.sequence < latest_stab_now {
                tracing::debug!(
                    region = %region.id,
                    seq = stab_sub.sequence,
                    latest = latest_stab_now,
                    "Discarding stale out-of-order translation response"
                );
                return;
            }
            let mut disp_map = self.latest_displayed_seq.lock();
            let prev_disp = disp_map.entry(region.id.clone()).or_insert(0);
            if stab_sub.sequence <= *prev_disp {
                return;
            }
            *prev_disp = stab_sub.sequence;
        }

        self.stabilizer
            .lock()
            .set_displayed_overlay_text(Some(&trans_ok.vietnamese_text));

        let latencies = StageLatenciesMs {
            capture_ms: frame.capture_latency_ms,
            ocr_ms: ocr_out.latency_ms,
            stabilization_ms: stab_sub.stabilization_ms,
            translation_ms: trans_ok.latency_ms,
            tts_synthesis_ms: None,
            first_audio_ms: None,
        };

        let tts_dev = self.tts.runtime_state().execution_device;
        let event = SubtitleEvent {
            region_id: region.id.clone(),
            sequence: stab_sub.sequence,
            source_language: region.source_language.clone(),
            original_text: stab_sub.text.clone(),
            vietnamese_text: trans_ok.vietnamese_text.clone(),
            provider: trans_ok.provider_used.clone(),
            from_cache: trans_ok.from_cache,
            ocr_device: ocr_out.inference_device.clone(),
            tts_device: tts_dev,
            latencies: latencies.clone(),
            timestamp_utc: Utc::now(),
        };

        *self.current_subtitle.write() = Some(event.clone());
        let _ = self.store.append_history(
            &event.region_id,
            &event.source_language,
            &event.original_text,
            &event.vietnamese_text,
            &event.provider,
        );

        let _ = self.events_tx.send(IpcServerMessage::SubtitleUpdated {
            subtitle: event.clone(),
        });

        if settings_snapshot.tts_enabled {
            {
                let mut map = self.utterance_to_subtitle.lock();
                if map.len() > 64 {
                    map.clear();
                }
            }

            match self.tts.speak_latest(&event.vietnamese_text).await {
                Ok(utt_id) if utt_id > 0 => {
                    self.utterance_to_subtitle
                        .lock()
                        .insert(utt_id, (event.region_id, event.sequence, latencies));
                }
                Ok(_) => {}
                Err(err) => {
                    let _ = self.events_tx.send(IpcServerMessage::PipelineError {
                        code: "TTS_ERROR".into(),
                        message_vi: format!("Lỗi đọc giọng nói cục bộ: {err}"),
                        detail: Some(err.to_string()),
                    });
                }
            }
        }
    }

    pub async fn shutdown_all(&self) {
        self.stop_monitoring().await;
        self.capturer.shutdown();
        self.ocr.shutdown().await;
        self.tts.shutdown().await;
        self.root_shutdown.cancel();
    }
}

#[async_trait]
impl IpcCommandHandler for PipelineCoordinator {
    async fn handle_request(&self, req: IpcRequest) -> IpcServerMessage {
        let req_id = req.request_id();
        match req {
            IpcRequest::GetState { .. } => IpcServerMessage::Response {
                request_id: req_id,
                ok: true,
                error: None,
                state: Some(self.snapshot()),
                monitors: None,
                history: None,
                test_translation_result: None,
            },
            IpcRequest::ListMonitors { .. } => {
                match self.capturer.list_monitors() {
                    Ok(mons) => IpcServerMessage::Response {
                        request_id: req_id,
                        ok: true,
                        error: None,
                        state: None,
                        monitors: Some(mons),
                        history: None,
                        test_translation_result: None,
                    },
                    Err(e) => error_response(req_id, e.to_string()),
                }
            }
            IpcRequest::UpsertRegion { region, .. } => {
                let mons: Vec<MonitorInfo> = self.capturer.list_monitors().unwrap_or_default();
                let validated = match validate_and_clamp_region(region, &mons) {
                    Ok(r) => r,
                    Err(e) => return error_response(req_id, e.to_string()),
                };
                {
                    let mut list = self.regions.write();
                    if let Some(existing) = list.iter_mut().find(|r| r.id == validated.id) {
                        *existing = validated.clone();
                    } else {
                        list.push(validated.clone());
                    }
                    let _ = self.store.save_regions(&list);
                }
                self.frame_diff.lock().remove_region(&validated.id);
                self.stabilizer.lock().remove_region(&validated.id);
                let snap = self.snapshot();
                ok_state_response(req_id, snap)
            }
            IpcRequest::DeleteRegion { region_id, .. } => {
                {
                    let mut list = self.regions.write();
                    list.retain(|r| r.id != region_id);
                    let _ = self.store.save_regions(&list);
                }
                self.frame_diff.lock().remove_region(&region_id);
                self.stabilizer.lock().remove_region(&region_id);
                let snap = self.snapshot();
                ok_state_response(req_id, snap)
            }
            IpcRequest::SetRegionEnabled {
                region_id, enabled, ..
            } => {
                {
                    let mut list = self.regions.write();
                    if let Some(r) = list.iter_mut().find(|r| r.id == region_id) {
                        r.enabled = enabled;
                    }
                    let _ = self.store.save_regions(&list);
                }
                if !enabled {
                    self.frame_diff.lock().remove_region(&region_id);
                    self.stabilizer.lock().remove_region(&region_id);
                }
                ok_state_response(req_id, self.snapshot())
            }
            IpcRequest::StartMonitoring { .. } => {
                // Upgrade self via temporary Arc wrapper? Wait: `handle_request` takes `&self`.
                // Let's handle StartMonitoring cleanly via `start_monitoring_from_ref`!
                error_response(req_id, "Internal handler dispatch mismatch".into())
            }
            IpcRequest::StopMonitoring { .. } => {
                self.stop_monitoring().await;
                ok_state_response(req_id, self.snapshot())
            }
            IpcRequest::UpdateSettings { settings, .. } => {
                let prev_tts_mode = self.settings.read().tts_execution_mode;
                let prev_tts_enabled = self.settings.read().tts_enabled;
                self.tts.set_volume(settings.tts_volume);
                if prev_tts_enabled && !settings.tts_enabled {
                    self.tts.stop_all().await;
                }
                *self.provider_status.write() =
                    format!("Sẵn sàng ({})", settings.active_provider.display_name());
                *self.settings.write() = settings.clone();
                let _ = self.store.save_settings(&settings);

                if settings.tts_execution_mode != prev_tts_mode {
                    let _ = self.tts.set_execution_mode(settings.tts_execution_mode).await;
                }
                ok_state_response(req_id, self.snapshot())
            }
            IpcRequest::RegisterOverlayWindow {
                hwnd,
                capture_excluded,
                x,
                y,
                width,
                height,
                ..
            } => {
                *self.overlay_info.write() = OverlayWindowInfo {
                    hwnd,
                    capture_excluded,
                    bounds: Some(RectPx {
                        x,
                        y,
                        width,
                        height,
                    }),
                };
                ok_state_response(req_id, self.snapshot())
            }
            IpcRequest::TestTranslation {
                provider,
                settings_override,
                ..
            } => {
                let eff = settings_override.unwrap_or_else(|| self.settings.read().clone());
                let kind = provider.unwrap_or(eff.active_provider);
                match self
                    .translator
                    .test_provider_connection(kind, &eff.credentials)
                    .await
                {
                    Ok((vi, latency_ms)) => IpcServerMessage::Response {
                        request_id: req_id,
                        ok: true,
                        error: None,
                        state: None,
                        monitors: None,
                        history: None,
                        test_translation_result: Some(TranslationTestResult {
                            provider: kind.display_name().to_string(),
                            sample_source: "Hello! Nice to meet you.".to_string(),
                            vietnamese_output: vi,
                            latency_ms,
                        }),
                    },
                    Err(e) => error_response(req_id, e.user_message_vi()),
                }
            }
            IpcRequest::QueryHistory {
                search_query,
                limit,
                offset,
                ..
            } => match self
                .store
                .query_history(search_query.as_deref(), limit, offset)
            {
                Ok(entries) => IpcServerMessage::Response {
                    request_id: req_id,
                    ok: true,
                    error: None,
                    state: None,
                    monitors: None,
                    history: Some(entries),
                    test_translation_result: None,
                },
                Err(e) => error_response(req_id, e.to_string()),
            },
            IpcRequest::ClearHistory { .. } => match self.store.clear_history() {
                Ok(()) => ok_state_response(req_id, self.snapshot()),
                Err(e) => error_response(req_id, e.to_string()),
            },
            IpcRequest::ClearCache { .. } => match self.store.clear_cache() {
                Ok(()) => ok_state_response(req_id, self.snapshot()),
                Err(e) => error_response(req_id, e.to_string()),
            },
            IpcRequest::Shutdown { .. } => {
                self.shutdown_all().await;
                IpcServerMessage::Response {
                    request_id: req_id,
                    ok: true,
                    error: None,
                    state: None,
                    monitors: None,
                    history: None,
                    test_translation_result: None,
                }
            }
        }
    }
}

/// Wrapper holding `Arc<PipelineCoordinator>` so `StartMonitoring` can clone the `Arc` cleanly.
pub struct CoordinatorIpcAdapter(pub Arc<PipelineCoordinator>);

#[async_trait]
impl IpcCommandHandler for CoordinatorIpcAdapter {
    async fn handle_request(&self, req: IpcRequest) -> IpcServerMessage {
        let req_id = req.request_id();
        if matches!(req, IpcRequest::StartMonitoring { .. }) {
            return match self.0.start_monitoring().await {
                Ok(()) => ok_state_response(req_id, self.0.snapshot()),
                Err(msg) => error_response(req_id, msg),
            };
        }
        self.0.handle_request(req).await
    }
}

fn ok_state_response(request_id: u64, state: AppStateSnapshot) -> IpcServerMessage {
    IpcServerMessage::Response {
        request_id,
        ok: true,
        error: None,
        state: Some(state),
        monitors: None,
        history: None,
        test_translation_result: None,
    }
}

fn error_response(request_id: u64, message: String) -> IpcServerMessage {
    IpcServerMessage::Response {
        request_id,
        ok: false,
        error: Some(message),
        state: None,
        monitors: None,
        history: None,
        test_translation_result: None,
    }
}

pub fn create_default_audio_player(initial_volume: f32) -> Arc<AudioPlayer> {
    Arc::new(AudioPlayer::new(initial_volume))
}
