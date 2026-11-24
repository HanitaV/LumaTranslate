//! Supervised local VieNeu-TTS v3 Turbo client & latest-subtitle-wins speech scheduler.
//! Streams 48 kHz `f32le` PCM chunks directly into `AudioPlayer` and supports immediate
//! mid-sentence interruption and cancellation when newer dialogue arrives.

use async_trait::async_trait;
use luma_audio::AudioPlayer;
use luma_core::{
    read_worker_frame, write_worker_frame, TtsExecutionMode, WorkerRuntimeState,
};
use parking_lot::RwLock;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{mpsc, Mutex};

#[derive(Debug, Error)]
pub enum TtsClientError {
    #[error("TTS worker launch failed: {0}")]
    LaunchFailed(String),
    #[error("TTS worker IPC error: {0}")]
    Ipc(String),
    #[error("TTS worker error: {0}")]
    WorkerError(String),
}

#[derive(Debug, Clone)]
pub struct SpeechTimingReport {
    pub utterance_id: u64,
    pub first_audio_ms: u64,
    pub total_synthesis_ms: u64,
    pub inference_device: String,
}

#[async_trait]
pub trait SpeechSynthesizer: Send + Sync {
    /// Queue Vietnamese subtitle text for immediate synthesis & playback using
    /// `latest-subtitle-wins` semantics.
    async fn speak_latest(&self, vietnamese_text: &str) -> Result<u64, TtsClientError>;
    async fn stop_all(&self);
    async fn set_execution_mode(&self, mode: TtsExecutionMode) -> Result<(), TtsClientError>;
    fn set_volume(&self, volume: f32);
    fn runtime_state(&self) -> WorkerRuntimeState;
    async fn shutdown(&self);
}

#[derive(Debug, Clone)]
pub struct TtsWorkerConfig {
    pub python_exe: PathBuf,
    pub worker_script: PathBuf,
    pub models_dir: PathBuf,
    pub execution_mode: TtsExecutionMode,
    pub voice: String,
    pub min_free_vram_mb: u64,
}

pub struct SupervisedTtsClient {
    config: RwLock<TtsWorkerConfig>,
    audio: Arc<AudioPlayer>,
    stdin_tx: Mutex<Option<ChildStdin>>,
    child_proc: Arc<Mutex<Option<Child>>>,
    state: Arc<RwLock<WorkerRuntimeState>>,
    latest_utterance_id: Arc<AtomicU64>,
    next_request_id: AtomicU64,
    last_spoken_text: RwLock<Option<String>>,
    timing_tx: Option<mpsc::Sender<SpeechTimingReport>>,
}

impl SupervisedTtsClient {
    pub fn new(
        config: TtsWorkerConfig,
        audio: Arc<AudioPlayer>,
        timing_tx: Option<mpsc::Sender<SpeechTimingReport>>,
    ) -> Self {
        Self {
            config: RwLock::new(config),
            audio,
            stdin_tx: Mutex::new(None),
            child_proc: Arc::new(Mutex::new(None)),
            state: Arc::new(RwLock::new(WorkerRuntimeState::default())),
            latest_utterance_id: Arc::new(AtomicU64::new(0)),
            next_request_id: AtomicU64::new(1),
            last_spoken_text: RwLock::new(None),
            timing_tx,
        }
    }

    pub async fn initialize(&self) -> Result<(), TtsClientError> {
        let mut stdin_guard = self.stdin_tx.lock().await;
        if stdin_guard.is_some() && self.state.read().ready {
            return Ok(());
        }
        self.spawn_worker_locked(&mut stdin_guard, false).await
    }

    async fn kill_existing_child(&self) {
        if let Some(mut old_child) = self.child_proc.lock().await.take() {
            let _ = old_child.kill().await;
            let _ = old_child.wait().await;
        }
    }

    async fn spawn_worker_locked(
        &self,
        stdin_slot: &mut Option<ChildStdin>,
        is_restart: bool,
    ) -> Result<(), TtsClientError> {
        *stdin_slot = None;
        self.kill_existing_child().await;

        let cfg = self.config.read().clone();
        let mut cmd = Command::new(&cfg.python_exe);
        cmd.arg("-u")
            .arg(&cfg.worker_script)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .env("PYTHONUTF8", "1")
            .env("PYTHONIOENCODING", "utf-8")
            .env("HF_HUB_DISABLE_SYMLINKS_WARNING", "1");

        #[cfg(windows)]
        {
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        let mut child = cmd
            .spawn()
            .map_err(|e| TtsClientError::LaunchFailed(format!("{}: {e}", cfg.python_exe.display())))?;

        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| TtsClientError::LaunchFailed("Missing TTS worker stdin".into()))?;
        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| TtsClientError::LaunchFailed("Missing TTS worker stdout".into()))?;

        if let Some(stderr) = child.stderr.take() {
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    tracing::debug!(target: "tts_worker_stderr", "{line}");
                }
            });
        }

        let init_req = serde_json::json!({
            "cmd": "init",
            "request_id": self.next_request_id.fetch_add(1, Ordering::Relaxed),
            "execution_mode": cfg.execution_mode.as_str(),
            "models_dir": cfg.models_dir.to_string_lossy(),
            "voice": cfg.voice,
            "min_free_vram_mb": cfg.min_free_vram_mb,
        });

        write_worker_frame(&mut stdin, &init_req, &[])
            .await
            .map_err(|e| TtsClientError::Ipc(e.to_string()))?;

        let (resp, _) = tokio::time::timeout(
            Duration::from_secs(180),
            read_worker_frame(&mut stdout),
        )
        .await
        .map_err(|_| TtsClientError::Ipc("TTS worker initialization timed out".into()))?
        .map_err(|e| TtsClientError::Ipc(e.to_string()))?;

        if resp.get("ok").and_then(|v| v.as_bool()) != Some(true) {
            let err = resp
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("TTS init failed")
                .to_string();
            let _ = child.kill().await;
            let mut st = self.state.write();
            st.ready = false;
            st.execution_device = "error".to_string();
            st.last_error = Some(err.clone());
            return Err(TtsClientError::WorkerError(err));
        }

        {
            let mut st = self.state.write();
            st.ready = true;
            st.execution_device = resp
                .get("device")
                .and_then(|v| v.as_str())
                .unwrap_or("cpu")
                .to_string();
            st.fallback_reason = resp
                .get("fallback_reason")
                .and_then(|v| v.as_str())
                .map(ToString::to_string);
            if is_restart {
                st.restart_count += 1;
            }
            st.last_error = None;
        }

        *self.child_proc.lock().await = Some(child);

        // Spawn asynchronous stdout reader streaming chunks directly to AudioPlayer
        let audio = Arc::clone(&self.audio);
        let state = Arc::clone(&self.state);
        let latest_id = Arc::clone(&self.latest_utterance_id);
        let timing_tx = self.timing_tx.clone();

        tokio::spawn(async move {
            let mut first_audio_timers: std::collections::HashMap<u64, Instant> =
                std::collections::HashMap::new();
            loop {
                match read_worker_frame(&mut stdout).await {
                    Ok((header, payload)) => {
                        let event = header.get("event").and_then(|v| v.as_str()).unwrap_or("");
                        let utt_id = header
                            .get("utterance_id")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0);

                        match event {
                            "audio_chunk" => {
                                // Discard stale audio chunks if a newer subtitle has superseded this utterance!
                                if utt_id == 0 || utt_id < latest_id.load(Ordering::Acquire) {
                                    continue;
                                }
                                let chunk_idx = header
                                    .get("chunk_index")
                                    .and_then(|v| v.as_u64())
                                    .unwrap_or(0);
                                let sr = header
                                    .get("sample_rate")
                                    .and_then(|v| v.as_u64())
                                    .unwrap_or(48000) as u32;
                                let channels = header
                                    .get("channels")
                                    .and_then(|v| v.as_u64())
                                    .unwrap_or(1) as u16;
                                if chunk_idx == 0 {
                                    first_audio_timers.insert(utt_id, Instant::now());
                                    let _ = audio.begin_utterance(utt_id);
                                }
                                let _ = audio.append_f32_le_bytes(utt_id, sr, channels, &payload);
                            }
                            "done" => {
                                first_audio_timers.remove(&utt_id);
                                let first_ms = header
                                    .get("first_chunk_latency_ms")
                                    .and_then(|v| v.as_u64())
                                    .unwrap_or(0);
                                let total_ms = header
                                    .get("latency_ms")
                                    .and_then(|v| v.as_u64())
                                    .unwrap_or(0);
                                let dev = header
                                    .get("inference_device")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("cuda")
                                    .to_string();
                                if let Some(fb) =
                                    header.get("fallback_reason").and_then(|v| v.as_str())
                                {
                                    let mut st = state.write();
                                    st.execution_device = dev.clone();
                                    st.fallback_reason = Some(fb.to_string());
                                }
                                if let Some(ref tx) = timing_tx {
                                    let _ = tx.try_send(SpeechTimingReport {
                                        utterance_id: utt_id,
                                        first_audio_ms: first_ms,
                                        total_synthesis_ms: total_ms,
                                        inference_device: dev,
                                    });
                                }
                            }
                            "cancelled" => {
                                first_audio_timers.remove(&utt_id);
                            }
                            _ => {
                                if header.get("ok").and_then(|v| v.as_bool()) == Some(false) {
                                    if let Some(err_str) =
                                        header.get("error").and_then(|v| v.as_str())
                                    {
                                        tracing::error!("TTS synthesis worker error: {err_str}");
                                        state.write().last_error = Some(err_str.to_string());
                                    }
                                }
                            }
                        }
                    }
                    Err(err) => {
                        tracing::warn!("TTS worker stdout closed/broken: {err}");
                        let mut st = state.write();
                        st.ready = false;
                        break;
                    }
                }
            }
        });

        *stdin_slot = Some(stdin);
        Ok(())
    }
}

#[async_trait]
impl SpeechSynthesizer for SupervisedTtsClient {
    async fn speak_latest(&self, vietnamese_text: &str) -> Result<u64, TtsClientError> {
        let cleaned = vietnamese_text.trim();
        if cleaned.is_empty() {
            return Ok(0);
        }

        // Latest-subtitle-wins: immediately stop obsolete playback and advance utterance ID.
        let utterance_id = self.latest_utterance_id.fetch_add(1, Ordering::AcqRel) + 1;
        self.audio.stop_immediately();
        *self.last_spoken_text.write() = Some(cleaned.to_string());

        let mut guard = self.stdin_tx.lock().await;
        if guard.is_none() || !self.state.read().ready {
            let is_restart = guard.is_some();
            self.spawn_worker_locked(&mut guard, is_restart).await?;
        }

        let req = serde_json::json!({
            "cmd": "synthesize",
            "request_id": self.next_request_id.fetch_add(1, Ordering::Relaxed),
            "utterance_id": utterance_id,
            "text": cleaned,
        });

        let send_res = if let Some(ref mut stdin) = *guard {
            write_worker_frame(stdin, &req, &[]).await
        } else {
            Err(luma_core::FramingError::Eof)
        };

        if send_res.is_err() {
            tracing::warn!("TTS worker pipe broken; restarting worker and retrying utterance");
            self.spawn_worker_locked(&mut guard, true).await?;
            if let Some(ref mut stdin) = *guard {
                write_worker_frame(stdin, &req, &[])
                    .await
                    .map_err(|e| TtsClientError::Ipc(e.to_string()))?;
            }
        }

        Ok(utterance_id)
    }

    async fn stop_all(&self) {
        let new_floor = self.latest_utterance_id.fetch_add(1, Ordering::AcqRel) + 1;
        self.audio.stop_immediately();
        *self.last_spoken_text.write() = None;

        let mut guard = self.stdin_tx.lock().await;
        if let Some(ref mut stdin) = *guard {
            let cancel_req = serde_json::json!({
                "cmd": "cancel",
                "request_id": self.next_request_id.fetch_add(1, Ordering::Relaxed),
                "utterance_id": new_floor,
            });
            let _ = write_worker_frame(stdin, &cancel_req, &[]).await;
        }
    }

    async fn set_execution_mode(&self, mode: TtsExecutionMode) -> Result<(), TtsClientError> {
        let changed = {
            let mut cfg = self.config.write();
            if cfg.execution_mode == mode {
                false
            } else {
                cfg.execution_mode = mode;
                true
            }
        };
        if !changed {
            return Ok(());
        }
        self.stop_all().await;
        let mut guard = self.stdin_tx.lock().await;
        if let Some(mut old_stdin) = guard.take() {
            let req = serde_json::json!({"cmd": "shutdown", "request_id": 0});
            let _ = write_worker_frame(&mut old_stdin, &req, &[]).await;
        }
        self.spawn_worker_locked(&mut guard, false).await
    }

    fn set_volume(&self, volume: f32) {
        self.audio.set_volume(volume);
    }

    fn runtime_state(&self) -> WorkerRuntimeState {
        self.state.read().clone()
    }

    async fn shutdown(&self) {
        self.stop_all().await;
        let mut guard = self.stdin_tx.lock().await;
        if let Some(mut stdin) = guard.take() {
            let req = serde_json::json!({
                "cmd": "shutdown",
                "request_id": self.next_request_id.fetch_add(1, Ordering::Relaxed),
            });
            let _ = write_worker_frame(&mut stdin, &req, &[]).await;
        }
        self.kill_existing_child().await;
        self.audio.shutdown();
        self.state.write().ready = false;
    }
}
