use anyhow::{Context, Result};
use luma_app::{create_default_audio_player, CoordinatorIpcAdapter, PipelineCoordinator};
use luma_capture::WgcD3d11Capturer;
use luma_core::IpcServerMessage;
use luma_ipc::{run_named_pipe_server, DEFAULT_PIPE_NAME};
use luma_ocr_client::{OcrBackend, OcrWorkerConfig, SupervisedOcrClient};
use luma_storage::LumaStore;
use luma_translation::CloudTranslationManager;
use luma_tts_client::{SpeechSynthesizer, SupervisedTtsClient, TtsWorkerConfig};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, mpsc};
use tokio_util::sync::CancellationToken;

#[derive(Debug)]
struct CliConfig {
    pipe_name: String,
    data_dir: PathBuf,
    models_dir: PathBuf,
    workers_dir: PathBuf,
    ocr_python: Option<PathBuf>,
    tts_python: Option<PathBuf>,
}

impl CliConfig {
    fn from_args() -> Self {
        let local_app = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        let app_root = local_app.join("LumaTranslate");

        let mut pipe_name = DEFAULT_PIPE_NAME.to_string();
        let mut data_dir = app_root.join("data");
        let mut models_dir = app_root.join("models");
        let mut workers_dir = discover_workers_dir();
        let mut ocr_python = std::env::var_os("LUMA_OCR_PYTHON").map(PathBuf::from);
        let mut tts_python = std::env::var_os("LUMA_TTS_PYTHON").map(PathBuf::from);

        let args: Vec<String> = std::env::args().collect();
        let mut idx = 1;
        while idx < args.len() {
            match args[idx].as_str() {
                "--pipe-name" if idx + 1 < args.len() => {
                    pipe_name = args[idx + 1].clone();
                    idx += 2;
                }
                "--data-dir" if idx + 1 < args.len() => {
                    data_dir = PathBuf::from(&args[idx + 1]);
                    idx += 2;
                }
                "--models-dir" if idx + 1 < args.len() => {
                    models_dir = PathBuf::from(&args[idx + 1]);
                    idx += 2;
                }
                "--workers-dir" if idx + 1 < args.len() => {
                    workers_dir = PathBuf::from(&args[idx + 1]);
                    idx += 2;
                }
                "--ocr-python" if idx + 1 < args.len() => {
                    ocr_python = Some(PathBuf::from(&args[idx + 1]));
                    idx += 2;
                }
                "--tts-python" if idx + 1 < args.len() => {
                    tts_python = Some(PathBuf::from(&args[idx + 1]));
                    idx += 2;
                }
                _ => {
                    idx += 1;
                }
            }
        }

        Self {
            pipe_name,
            data_dir,
            models_dir,
            workers_dir,
            ocr_python,
            tts_python,
        }
    }
}

fn discover_workers_dir() -> PathBuf {
    if let Ok(env_dir) = std::env::var("LUMA_WORKERS_DIR") {
        let p = PathBuf::from(env_dir);
        if p.exists() {
            return p;
        }
    }

    if let Ok(exe) = std::env::current_exe() {
        let mut cur: Option<&Path> = exe.parent();
        while let Some(dir) = cur {
            let candidate = dir.join("workers");
            if candidate.join("ocr").join("worker.py").exists() {
                return candidate;
            }
            cur = dir.parent();
        }
    }

    PathBuf::from("workers")
}

fn resolve_python_for_worker(
    explicit: Option<&Path>,
    worker_name: &str,
    worker_subdir: &Path,
) -> PathBuf {
    if let Some(custom) = explicit {
        if custom.exists() {
            return custom.to_path_buf();
        }
    }

    let venv_py = worker_subdir.join(".venv").join("Scripts").join("python.exe");
    if venv_py.exists() {
        return venv_py;
    }

    if let Some(local_app) = std::env::var_os("LOCALAPPDATA") {
        let managed_py = PathBuf::from(local_app)
            .join("LumaTranslate")
            .join("runtimes")
            .join(worker_name)
            .join(".venv")
            .join("Scripts")
            .join("python.exe");
        if managed_py.exists() {
            return managed_py;
        }
    }

    let bundled_py = worker_subdir.join("runtime").join("python.exe");
    if bundled_py.exists() {
        return bundled_py;
    }
    PathBuf::from("python")
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();

    let cfg = CliConfig::from_args();
    tracing::info!(
        pipe = %cfg.pipe_name,
        data_dir = %cfg.data_dir.display(),
        workers_dir = %cfg.workers_dir.display(),
        "Starting LumaTranslate.Backend"
    );

    let store = Arc::new(
        LumaStore::open_in_dir(&cfg.data_dir).context("Failed to open SQLite store")?,
    );
    let saved_settings = store.load_settings().ok().flatten().unwrap_or_default();

    let capturer = Arc::new(
        WgcD3d11Capturer::new().context("Failed to initialize Windows Graphics Capture + D3D11")?,
    );

    let ocr_dir = cfg.workers_dir.join("ocr");
    let ocr_client = Arc::new(SupervisedOcrClient::new(OcrWorkerConfig {
        python_exe: resolve_python_for_worker(cfg.ocr_python.as_deref(), "ocr", &ocr_dir),
        worker_script: ocr_dir.join("worker.py"),
        models_dir: cfg.models_dir.join("paddleocr"),
        preferred_device: "cuda".to_string(),
        min_free_vram_mb: 650,
        request_timeout: Duration::from_secs(15),
    }));

    let audio_player = create_default_audio_player(saved_settings.tts_volume);
    let (timing_tx, timing_rx) = mpsc::channel(32);

    let tts_dir = cfg.workers_dir.join("tts");
    let tts_client = Arc::new(SupervisedTtsClient::new(
        TtsWorkerConfig {
            python_exe: resolve_python_for_worker(cfg.tts_python.as_deref(), "tts", &tts_dir),
            worker_script: tts_dir.join("worker.py"),
            models_dir: cfg.models_dir.join("tts"),
            execution_mode: saved_settings.tts_execution_mode,
            voice: "Hải Đăng".to_string(),
            min_free_vram_mb: 1400,
        },
        audio_player,
        Some(timing_tx),
    ));

    let translator = Arc::new(CloudTranslationManager::new(Arc::clone(&store)));
    let (events_tx, _) = broadcast::channel::<IpcServerMessage>(128);
    let root_shutdown = CancellationToken::new();

    let coordinator = PipelineCoordinator::new(
        store,
        capturer,
        ocr_client.clone(),
        translator,
        tts_client.clone(),
        events_tx.clone(),
        root_shutdown.clone(),
        Some(timing_rx),
    );

    // Warm up OCR and TTS workers concurrently at startup so Named Pipe connects immediately
    // and both workers reach READY in ~28s instead of ~52s sequential!
    {
        let ocr_for_ocr = Arc::clone(&ocr_client);
        let tts_for_ocr = Arc::clone(&tts_client);
        let coord_for_ocr = Arc::clone(&coordinator);
        let ev_for_ocr = events_tx.clone();
        tokio::spawn(async move {
            if let Err(err) = ocr_for_ocr.initialize().await {
                tracing::error!("OCR worker startup init failed: {err}");
                let _ = ev_for_ocr.send(IpcServerMessage::PipelineError {
                    code: "OCR_INIT_ERROR".into(),
                    message_vi: format!("Lỗi khởi tạo mô hình OCR: {err}"),
                    detail: Some(err.to_string()),
                });
            }
            let _ = ev_for_ocr.send(IpcServerMessage::WorkerStatus {
                ocr_worker: ocr_for_ocr.runtime_state(),
                tts_worker: tts_for_ocr.runtime_state(),
            });
            coord_for_ocr.broadcast_snapshot();
        });

        let ocr_for_tts = Arc::clone(&ocr_client);
        let tts_for_tts = Arc::clone(&tts_client);
        let coord_for_tts = Arc::clone(&coordinator);
        let ev_for_tts = events_tx.clone();
        tokio::spawn(async move {
            // Brief stagger so OCR reserves its ~600MB CUDA context before TTS inspects free VRAM
            tokio::time::sleep(Duration::from_millis(1200)).await;
            if let Err(err) = tts_for_tts.initialize().await {
                tracing::error!("TTS worker startup init failed: {err}");
                let _ = ev_for_tts.send(IpcServerMessage::PipelineError {
                    code: "TTS_INIT_ERROR".into(),
                    message_vi: format!("Lỗi khởi tạo giọng đọc VieNeu-TTS: {err}"),
                    detail: Some(err.to_string()),
                });
            }
            let _ = ev_for_tts.send(IpcServerMessage::WorkerStatus {
                ocr_worker: ocr_for_tts.runtime_state(),
                tts_worker: tts_for_tts.runtime_state(),
            });
            coord_for_tts.broadcast_snapshot();
        });
    }

    let shutdown_signal = root_shutdown.clone();
    let coord_ctrlc = Arc::clone(&coordinator);
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            tracing::info!("Ctrl+C received; shutting down backend and AI workers");
            coord_ctrlc.shutdown_all().await;
            shutdown_signal.cancel();
        }
    });

    run_named_pipe_server(
        cfg.pipe_name,
        Arc::new(CoordinatorIpcAdapter(Arc::clone(&coordinator))),
        events_tx,
        root_shutdown,
    )
    .await
    .context("Named pipe IPC server failed")?;

    coordinator.shutdown_all().await;
    tracing::info!("LumaTranslate.Backend exited cleanly");
    Ok(())
}
