//! Supervised Python PaddleOCR PP-OCRv5 Mobile client over private framed binary/JSON stdio.
//! Enforces bounded queues, drops stale per-region frames, logs diagnostics from stderr,
//! and automatically restarts crashed worker processes.

use async_trait::async_trait;
use luma_core::{
    read_worker_frame, write_worker_frame, GpuTelemetry, OcrRecognitionOutput, WorkerRuntimeState,
};
use parking_lot::RwLock;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;

#[derive(Debug, Error)]
pub enum OcrClientError {
    #[error("OCR worker process launch failed: {0}")]
    LaunchFailed(String),
    #[error("OCR worker IPC failure: {0}")]
    Ipc(String),
    #[error("OCR worker reported error: {0}")]
    WorkerError(String),
    #[error("OCR request timed out")]
    Timeout,
}

#[async_trait]
pub trait OcrBackend: Send + Sync {
    async fn recognize_bgra8(
        &self,
        region_id: &str,
        sequence: u64,
        source_language: &str,
        width: u32,
        height: u32,
        stride: u32,
        bgra_bytes: &[u8],
    ) -> Result<OcrRecognitionOutput, OcrClientError>;

    fn runtime_state(&self) -> WorkerRuntimeState;
    fn gpu_telemetry(&self) -> GpuTelemetry;
    async fn shutdown(&self);
}

#[derive(Debug, Clone)]
pub struct OcrWorkerConfig {
    pub python_exe: PathBuf,
    pub worker_script: PathBuf,
    pub models_dir: PathBuf,
    pub preferred_device: String,
    pub min_free_vram_mb: u64,
    pub request_timeout: Duration,
}

struct RunningOcrChild {
    child: Child,
    stdin: ChildStdin,
    stdout: ChildStdout,
}

pub struct SupervisedOcrClient {
    config: OcrWorkerConfig,
    io_lock: Mutex<Option<RunningOcrChild>>,
    state: Arc<RwLock<WorkerRuntimeState>>,
    gpu: Arc<RwLock<GpuTelemetry>>,
    next_request_id: AtomicU64,
}

impl SupervisedOcrClient {
    pub fn new(config: OcrWorkerConfig) -> Self {
        Self {
            config,
            io_lock: Mutex::new(None),
            state: Arc::new(RwLock::new(WorkerRuntimeState::default())),
            gpu: Arc::new(RwLock::new(GpuTelemetry::default())),
            next_request_id: AtomicU64::new(1),
        }
    }

    pub async fn initialize(&self) -> Result<(), OcrClientError> {
        let mut guard = self.io_lock.lock().await;
        self.spawn_and_init_locked(&mut guard, false).await
    }

    async fn spawn_and_init_locked(
        &self,
        slot: &mut Option<RunningOcrChild>,
        is_restart: bool,
    ) -> Result<(), OcrClientError> {
        if let Some(mut old) = slot.take() {
            let _ = old.child.kill().await;
        }

        let mut cmd = Command::new(&self.config.python_exe);
        cmd.arg("-u")
            .arg(&self.config.worker_script)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .env("PYTHONUTF8", "1")
            .env("PYTHONIOENCODING", "utf-8")
            .env("PADDLE_PDX_DISABLE_MODEL_SOURCE_CHECK", "True");

        #[cfg(windows)]
        {
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        let mut child = cmd
            .spawn()
            .map_err(|e| OcrClientError::LaunchFailed(format!("{}: {e}", self.config.python_exe.display())))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| OcrClientError::LaunchFailed("Missing worker stdin".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| OcrClientError::LaunchFailed("Missing worker stdout".into()))?;

        if let Some(stderr) = child.stderr.take() {
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    tracing::debug!(target: "ocr_worker_stderr", "{line}");
                }
            });
        }

        let mut running = RunningOcrChild {
            child,
            stdin,
            stdout,
        };

        let req_id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
        let init_req = serde_json::json!({
            "cmd": "init",
            "request_id": req_id,
            "preferred_device": self.config.preferred_device,
            "models_dir": self.config.models_dir.to_string_lossy(),
            "min_free_vram_mb": self.config.min_free_vram_mb,
        });

        write_worker_frame(&mut running.stdin, &init_req, &[])
            .await
            .map_err(|e| OcrClientError::Ipc(e.to_string()))?;

        let (resp, _) = tokio::time::timeout(
            Duration::from_secs(120),
            read_worker_frame(&mut running.stdout),
        )
        .await
        .map_err(|_| OcrClientError::Timeout)?
        .map_err(|e| OcrClientError::Ipc(e.to_string()))?;

        if resp.get("ok").and_then(|v| v.as_bool()) != Some(true) {
            let err_msg = resp
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown OCR init failure")
                .to_string();
            let _ = running.child.kill().await;
            let mut st = self.state.write();
            st.ready = false;
            st.execution_device = "error".to_string();
            st.last_error = Some(err_msg.clone());
            return Err(OcrClientError::WorkerError(err_msg));
        }

        let device = resp
            .get("device")
            .and_then(|v| v.as_str())
            .unwrap_or("cpu")
            .to_string();
        let fallback = resp
            .get("fallback_reason")
            .and_then(|v| v.as_str())
            .map(ToString::to_string);

        {
            let mut gpu = self.gpu.write();
            gpu.gpu_available = resp
                .get("gpu_available")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            gpu.gpu_name = resp
                .get("gpu_name")
                .and_then(|v| v.as_str())
                .map(ToString::to_string);
            gpu.driver_version = resp
                .get("driver_version")
                .and_then(|v| v.as_str())
                .map(ToString::to_string);
            gpu.vram_total_mb = resp
                .get("vram_total_mb")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            gpu.vram_used_mb = resp
                .get("vram_used_mb")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            gpu.vram_free_mb = resp
                .get("vram_free_mb")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            gpu.gpu_utilization_pct = resp
                .get("gpu_utilization_pct")
                .and_then(|v| v.as_u64())
                .map(|v| v as u32);
        }

        {
            let mut st = self.state.write();
            st.ready = true;
            st.execution_device = device;
            st.fallback_reason = fallback;
            if is_restart {
                st.restart_count += 1;
            }
            st.last_error = None;
        }

        *slot = Some(running);
        Ok(())
    }

    async fn send_ocr_once(
        &self,
        running: &mut RunningOcrChild,
        region_id: &str,
        sequence: u64,
        source_language: &str,
        width: u32,
        height: u32,
        stride: u32,
        bgra_bytes: &[u8],
    ) -> Result<OcrRecognitionOutput, OcrClientError> {
        let req_id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
        let req = serde_json::json!({
            "cmd": "ocr",
            "request_id": req_id,
            "region_id": region_id,
            "sequence": sequence,
            "source_language": source_language,
            "pixel_format": "bgra8",
            "width": width,
            "height": height,
            "stride": stride,
        });

        write_worker_frame(&mut running.stdin, &req, bgra_bytes)
            .await
            .map_err(|e| OcrClientError::Ipc(e.to_string()))?;

        let (resp, _) = tokio::time::timeout(
            self.config.request_timeout,
            read_worker_frame(&mut running.stdout),
        )
        .await
        .map_err(|_| OcrClientError::Timeout)?
        .map_err(|e| OcrClientError::Ipc(e.to_string()))?;

        if resp.get("ok").and_then(|v| v.as_bool()) != Some(true) {
            let err_msg = resp
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("OCR worker execution failed")
                .to_string();
            return Err(OcrClientError::WorkerError(err_msg));
        }

        let dev = resp
            .get("inference_device")
            .and_then(|v| v.as_str())
            .unwrap_or("cpu")
            .to_string();
        if let Some(fb) = resp.get("fallback_reason").and_then(|v| v.as_str()) {
            let mut st = self.state.write();
            st.execution_device = dev.clone();
            st.fallback_reason = Some(fb.to_string());
        }

        Ok(OcrRecognitionOutput {
            region_id: resp
                .get("region_id")
                .and_then(|v| v.as_str())
                .unwrap_or(region_id)
                .to_string(),
            sequence: resp
                .get("sequence")
                .and_then(|v| v.as_u64())
                .unwrap_or(sequence),
            source_language: resp
                .get("source_language")
                .and_then(|v| v.as_str())
                .unwrap_or(source_language)
                .to_string(),
            text: resp
                .get("text")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            confidence: resp
                .get("confidence")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0) as f32,
            inference_device: dev,
            latency_ms: resp
                .get("latency_ms")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
        })
    }
}

#[async_trait]
impl OcrBackend for SupervisedOcrClient {
    async fn recognize_bgra8(
        &self,
        region_id: &str,
        sequence: u64,
        source_language: &str,
        width: u32,
        height: u32,
        stride: u32,
        bgra_bytes: &[u8],
    ) -> Result<OcrRecognitionOutput, OcrClientError> {
        let mut guard = self.io_lock.lock().await;
        if guard.is_none() {
            self.spawn_and_init_locked(&mut guard, false).await?;
        }

        let first_attempt = {
            let running = guard.as_mut().unwrap();
            self.send_ocr_once(
                running,
                region_id,
                sequence,
                source_language,
                width,
                height,
                stride,
                bgra_bytes,
            )
            .await
        };

        match first_attempt {
            Ok(out) => Ok(out),
            Err(OcrClientError::Ipc(e)) => {
                tracing::warn!("OCR worker crashed or disconnected ({e}); restarting once");
                self.spawn_and_init_locked(&mut guard, true).await?;
                let running = guard.as_mut().unwrap();
                self.send_ocr_once(
                    running,
                    region_id,
                    sequence,
                    source_language,
                    width,
                    height,
                    stride,
                    bgra_bytes,
                )
                .await
            }
            Err(other) => Err(other),
        }
    }

    fn runtime_state(&self) -> WorkerRuntimeState {
        self.state.read().clone()
    }

    fn gpu_telemetry(&self) -> GpuTelemetry {
        self.gpu.read().clone()
    }

    async fn shutdown(&self) {
        let mut guard = self.io_lock.lock().await;
        if let Some(mut running) = guard.take() {
            let req = serde_json::json!({
                "cmd": "shutdown",
                "request_id": self.next_request_id.fetch_add(1, Ordering::Relaxed),
            });
            let _ = write_worker_frame(&mut running.stdin, &req, &[]).await;
            let _ = tokio::time::timeout(Duration::from_secs(3), running.child.wait()).await;
            let _ = running.child.kill().await;
        }
        self.state.write().ready = false;
    }
}
