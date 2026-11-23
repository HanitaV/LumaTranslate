//! Windows Named Pipe server (`\\.\pipe\LumaTranslate.IPC`) bridging the WinUI 3 frontend
//! and the Rust backend using length-prefixed UTF-8 JSON frames.

use async_trait::async_trait;
use luma_core::{read_ipc_json, write_ipc_json, FramingError, IpcRequest, IpcServerMessage};
use std::sync::Arc;
use tokio::io::split;
use tokio::net::windows::named_pipe::ServerOptions;
use tokio::sync::{broadcast, mpsc};
use tokio_util::sync::CancellationToken;

pub const DEFAULT_PIPE_NAME: &str = r"\\.\pipe\LumaTranslate.IPC";

#[async_trait]
pub trait IpcCommandHandler: Send + Sync + 'static {
    async fn handle_request(&self, req: IpcRequest) -> IpcServerMessage;
}

pub async fn run_named_pipe_server(
    pipe_name: String,
    handler: Arc<dyn IpcCommandHandler>,
    events_tx: broadcast::Sender<IpcServerMessage>,
    shutdown_token: CancellationToken,
) -> std::io::Result<()> {
    let mut is_first = true;

    loop {
        if shutdown_token.is_cancelled() {
            break;
        }

        let server = ServerOptions::new()
            .first_pipe_instance(is_first)
            .in_buffer_size(256 * 1024)
            .out_buffer_size(256 * 1024)
            .create(&pipe_name)?;
        is_first = false;

        tokio::select! {
            _ = shutdown_token.cancelled() => break,
            conn_res = server.connect() => {
                if let Err(err) = conn_res {
                    tracing::warn!("Named pipe connect error on {pipe_name}: {err}");
                    continue;
                }
            }
        }

        let handler_clone = Arc::clone(&handler);
        let mut events_rx = events_tx.subscribe();
        let client_cancel = shutdown_token.child_token();

        tokio::spawn(async move {
            let (mut pipe_reader, mut pipe_writer) = split(server);
            let (out_tx, mut out_rx) = mpsc::channel::<IpcServerMessage>(64);

            let out_tx_events = out_tx.clone();
            let cancel_events = client_cancel.clone();
            let event_forwarder = tokio::spawn(async move {
                loop {
                    tokio::select! {
                        _ = cancel_events.cancelled() => break,
                        ev = events_rx.recv() => {
                            match ev {
                                Ok(msg) => {
                                    if out_tx_events.send(msg).await.is_err() {
                                        break;
                                    }
                                }
                                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                                    tracing::warn!("IPC client lagged by {skipped} events");
                                }
                                Err(broadcast::error::RecvError::Closed) => break,
                            }
                        }
                    }
                }
            });

            let writer_task = tokio::spawn(async move {
                while let Some(msg) = out_rx.recv().await {
                    if write_ipc_json(&mut pipe_writer, &msg).await.is_err() {
                        break;
                    }
                }
            });

            loop {
                let req_res: Result<IpcRequest, FramingError> = tokio::select! {
                    _ = client_cancel.cancelled() => break,
                    r = read_ipc_json(&mut pipe_reader) => r,
                };

                match req_res {
                    Ok(req) => {
                        let is_shutdown = matches!(req, IpcRequest::Shutdown { .. });
                        let resp = handler_clone.handle_request(req).await;
                        if out_tx.send(resp).await.is_err() {
                            break;
                        }
                        if is_shutdown {
                            break;
                        }
                    }
                    Err(FramingError::Eof) => break,
                    Err(err) => {
                        tracing::debug!("IPC client disconnected or invalid frame: {err}");
                        break;
                    }
                }
            }

            client_cancel.cancel();
            drop(out_tx);
            let _ = event_forwarder.await;
            let _ = writer_task.await;
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::windows::named_pipe::ClientOptions;

    struct EchoStateHandler;

    #[async_trait]
    impl IpcCommandHandler for EchoStateHandler {
        async fn handle_request(&self, req: IpcRequest) -> IpcServerMessage {
            IpcServerMessage::Response {
                request_id: req.request_id(),
                ok: true,
                error: None,
                state: None,
                monitors: Some(vec![]),
                history: None,
                test_translation_result: None,
            }
        }
    }

    #[tokio::test]
    async fn named_pipe_ipc_roundtrip_and_event_broadcast_works() {
        let pipe_name = format!(r"\\.\pipe\LumaTranslate.Test.{}", std::process::id());
        let (events_tx, _) = broadcast::channel(16);
        let cancel = CancellationToken::new();

        let srv_pipe = pipe_name.clone();
        let srv_tx = events_tx.clone();
        let srv_cancel = cancel.clone();
        let srv_handle = tokio::spawn(async move {
            run_named_pipe_server(srv_pipe, Arc::new(EchoStateHandler), srv_tx, srv_cancel)
                .await
                .unwrap();
        });

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let mut client = ClientOptions::new().open(&pipe_name).unwrap();

        write_ipc_json(&mut client, &IpcRequest::ListMonitors { request_id: 99 })
            .await
            .unwrap();
        let reply: IpcServerMessage = read_ipc_json(&mut client).await.unwrap();
        match reply {
            IpcServerMessage::Response { request_id, ok, .. } => {
                assert_eq!(request_id, 99);
                assert!(ok);
            }
            other => panic!("Unexpected reply: {other:?}"),
        }

        // Server broadcast event reaches connected client
        events_tx
            .send(IpcServerMessage::PipelineError {
                code: "TEST".into(),
                message_vi: "Kiểm thử ống dẫn".into(),
                detail: None,
            })
            .unwrap();

        let pushed: IpcServerMessage = read_ipc_json(&mut client).await.unwrap();
        assert!(matches!(pushed, IpcServerMessage::PipelineError { .. }));

        cancel.cancel();
        drop(client);
        let _ = srv_handle.await;
    }
}
