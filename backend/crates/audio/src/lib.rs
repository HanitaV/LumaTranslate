//! Low-latency in-memory Windows WASAPI audio player coordinated by the Rust backend.
//! Supports streaming mono `f32` PCM chunks, volume control, and instant interruption
//! when a newer subtitle supersedes the active utterance.

use parking_lot::Mutex;
use rodio::{buffer::SamplesBuffer, OutputStream, OutputStreamHandle, Sink};
use std::sync::mpsc;
use std::thread;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AudioError {
    #[error("Audio playback thread is unavailable")]
    PlaybackUnavailable,
}

enum AudioCmd {
    BeginUtterance { utterance_id: u64, volume: f32 },
    AppendSamples {
        utterance_id: u64,
        sample_rate: u32,
        channels: u16,
        samples: Vec<f32>,
    },
    SetVolume(f32),
    StopCurrent,
    Shutdown,
}

pub struct AudioPlayer {
    tx: mpsc::Sender<AudioCmd>,
    active_utterance: Mutex<u64>,
    volume: Mutex<f32>,
    has_device: bool,
}

impl AudioPlayer {
    pub fn new(initial_volume: f32) -> Self {
        let (tx, rx) = mpsc::channel::<AudioCmd>();
        let (init_tx, init_rx) = mpsc::sync_channel::<bool>(1);

        thread::Builder::new()
            .name("luma-wasapi-audio".to_string())
            .spawn(move || {
                let stream_pair: Option<(OutputStream, OutputStreamHandle)> =
                    OutputStream::try_default().ok();
                let _ = init_tx.send(stream_pair.is_some());

                let mut current_sink: Option<Sink> = None;
                let mut current_utterance: u64 = 0;
                let mut current_volume: f32 = initial_volume.clamp(0.0, 1.5);

                while let Ok(cmd) = rx.recv() {
                    match cmd {
                        AudioCmd::BeginUtterance {
                            utterance_id,
                            volume,
                        } => {
                            if let Some(old_sink) = current_sink.take() {
                                old_sink.stop();
                            }
                            current_utterance = utterance_id;
                            current_volume = volume.clamp(0.0, 1.5);
                            if let Some((_, ref handle)) = stream_pair {
                                if let Ok(sink) = Sink::try_new(handle) {
                                    sink.set_volume(current_volume);
                                    current_sink = Some(sink);
                                }
                            }
                        }
                        AudioCmd::AppendSamples {
                            utterance_id,
                            sample_rate,
                            channels,
                            samples,
                        } => {
                            if utterance_id != current_utterance || samples.is_empty() {
                                continue;
                            }
                            if current_sink.is_none() {
                                if let Some((_, ref handle)) = stream_pair {
                                    if let Ok(sink) = Sink::try_new(handle) {
                                        sink.set_volume(current_volume);
                                        current_sink = Some(sink);
                                    }
                                }
                            }
                            if let Some(ref sink) = current_sink {
                                let buf = SamplesBuffer::new(
                                    channels.max(1),
                                    sample_rate.max(8000),
                                    samples,
                                );
                                sink.append(buf);
                            }
                        }
                        AudioCmd::SetVolume(vol) => {
                            current_volume = vol.clamp(0.0, 1.5);
                            if let Some(ref sink) = current_sink {
                                sink.set_volume(current_volume);
                            }
                        }
                        AudioCmd::StopCurrent => {
                            current_utterance = 0;
                            if let Some(old_sink) = current_sink.take() {
                                old_sink.stop();
                            }
                        }
                        AudioCmd::Shutdown => {
                            if let Some(old_sink) = current_sink.take() {
                                old_sink.stop();
                            }
                            break;
                        }
                    }
                }
            })
            .expect("Failed to spawn audio thread");

        let has_device = init_rx.recv().unwrap_or(false);
        if !has_device {
            tracing::warn!("No default Windows audio output device detected; running in silent audio mode");
        }

        Self {
            tx,
            active_utterance: Mutex::new(0),
            volume: Mutex::new(initial_volume.clamp(0.0, 1.5)),
            has_device,
        }
    }

    pub fn has_audio_device(&self) -> bool {
        self.has_device
    }

    pub fn begin_utterance(&self, utterance_id: u64) -> Result<(), AudioError> {
        let vol = *self.volume.lock();
        *self.active_utterance.lock() = utterance_id;
        self.tx
            .send(AudioCmd::BeginUtterance {
                utterance_id,
                volume: vol,
            })
            .map_err(|_| AudioError::PlaybackUnavailable)
    }

    pub fn append_f32_le_bytes(
        &self,
        utterance_id: u64,
        sample_rate: u32,
        channels: u16,
        pcm_bytes: &[u8],
    ) -> Result<(), AudioError> {
        if *self.active_utterance.lock() != utterance_id {
            return Ok(());
        }
        let sample_count = pcm_bytes.len() / 4;
        if sample_count == 0 {
            return Ok(());
        }
        let mut samples = Vec::with_capacity(sample_count);
        for chunk in pcm_bytes.chunks_exact(4) {
            let val = f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            samples.push(if val.is_finite() {
                val.clamp(-1.0, 1.0)
            } else {
                0.0
            });
        }
        self.tx
            .send(AudioCmd::AppendSamples {
                utterance_id,
                sample_rate,
                channels,
                samples,
            })
            .map_err(|_| AudioError::PlaybackUnavailable)
    }

    pub fn set_volume(&self, volume: f32) {
        let clamped = volume.clamp(0.0, 1.5);
        *self.volume.lock() = clamped;
        let _ = self.tx.send(AudioCmd::SetVolume(clamped));
    }

    pub fn stop_immediately(&self) {
        *self.active_utterance.lock() = 0;
        let _ = self.tx.send(AudioCmd::StopCurrent);
    }

    pub fn shutdown(&self) {
        *self.active_utterance.lock() = 0;
        let _ = self.tx.send(AudioCmd::Shutdown);
    }
}

impl Drop for AudioPlayer {
    fn drop(&mut self) {
        let _ = self.tx.send(AudioCmd::Shutdown);
    }
}
