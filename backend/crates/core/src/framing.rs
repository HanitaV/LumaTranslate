//! Framed binary + UTF-8 JSON codec for private local child-process IPC
//! and length-prefixed JSON codec for Named Pipe IPC.

use serde::{de::DeserializeOwned, Serialize};
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub const MAX_WORKER_JSON_BYTES: usize = 1024 * 1024;
pub const MAX_WORKER_BINARY_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_IPC_MESSAGE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum FramingError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Clean stream EOF")]
    Eof,
    #[error("Frame header length {0} out of bounds")]
    InvalidHeaderLength(usize),
    #[error("Frame binary payload length {0} out of bounds")]
    InvalidPayloadLength(usize),
    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),
}

pub async fn write_worker_frame<W, T>(
    writer: &mut W,
    header: &T,
    payload: &[u8],
) -> Result<(), FramingError>
where
    W: AsyncWrite + Unpin,
    T: Serialize,
{
    let json_bytes = serde_json::to_vec(header)?;
    if json_bytes.is_empty() || json_bytes.len() > MAX_WORKER_JSON_BYTES {
        return Err(FramingError::InvalidHeaderLength(json_bytes.len()));
    }
    if payload.len() > MAX_WORKER_BINARY_BYTES {
        return Err(FramingError::InvalidPayloadLength(payload.len()));
    }

    let mut prefix = [0u8; 8];
    prefix[0..4].copy_from_slice(&(json_bytes.len() as u32).to_le_bytes());
    prefix[4..8].copy_from_slice(&(payload.len() as u32).to_le_bytes());

    writer.write_all(&prefix).await?;
    writer.write_all(&json_bytes).await?;
    if !payload.is_empty() {
        writer.write_all(payload).await?;
    }
    writer.flush().await?;
    Ok(())
}

pub async fn read_worker_frame<R>(
    reader: &mut R,
) -> Result<(serde_json::Value, Vec<u8>), FramingError>
where
    R: AsyncRead + Unpin,
{
    let mut prefix = [0u8; 8];
    match reader.read_exact(&mut prefix).await {
        Ok(_) => {}
        Err(err) if err.kind() == std::io::ErrorKind::UnexpectedEof => {
            return Err(FramingError::Eof);
        }
        Err(err) => return Err(FramingError::Io(err)),
    }

    let json_len = u32::from_le_bytes([prefix[0], prefix[1], prefix[2], prefix[3]]) as usize;
    let payload_len = u32::from_le_bytes([prefix[4], prefix[5], prefix[6], prefix[7]]) as usize;

    if json_len == 0 || json_len > MAX_WORKER_JSON_BYTES {
        return Err(FramingError::InvalidHeaderLength(json_len));
    }
    if payload_len > MAX_WORKER_BINARY_BYTES {
        return Err(FramingError::InvalidPayloadLength(payload_len));
    }

    let mut json_buf = vec![0u8; json_len];
    reader.read_exact(&mut json_buf).await?;

    let mut payload_buf = vec![0u8; payload_len];
    if payload_len > 0 {
        reader.read_exact(&mut payload_buf).await?;
    }

    let header: serde_json::Value = serde_json::from_slice(&json_buf)?;
    Ok((header, payload_buf))
}

pub async fn write_ipc_json<W, T>(writer: &mut W, msg: &T) -> Result<(), FramingError>
where
    W: AsyncWrite + Unpin,
    T: Serialize,
{
    let bytes = serde_json::to_vec(msg)?;
    if bytes.is_empty() || bytes.len() > MAX_IPC_MESSAGE_BYTES {
        return Err(FramingError::InvalidHeaderLength(bytes.len()));
    }
    let len_prefix = (bytes.len() as u32).to_le_bytes();
    writer.write_all(&len_prefix).await?;
    writer.write_all(&bytes).await?;
    writer.flush().await?;
    Ok(())
}

pub async fn read_ipc_json<R, T>(reader: &mut R) -> Result<T, FramingError>
where
    R: AsyncRead + Unpin,
    T: DeserializeOwned,
{
    let mut len_buf = [0u8; 4];
    match reader.read_exact(&mut len_buf).await {
        Ok(_) => {}
        Err(err) if err.kind() == std::io::ErrorKind::UnexpectedEof => {
            return Err(FramingError::Eof);
        }
        Err(err) => return Err(FramingError::Io(err)),
    }
    let len = u32::from_le_bytes(len_buf) as usize;
    if len == 0 || len > MAX_IPC_MESSAGE_BYTES {
        return Err(FramingError::InvalidHeaderLength(len));
    }
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body).await?;
    let parsed = serde_json::from_slice(&body)?;
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn worker_frame_roundtrip_preserves_json_and_binary_payload() {
        let (mut client, mut server) = tokio::io::duplex(64 * 1024);
        let req = serde_json::json!({"cmd": "ocr", "sequence": 7});
        let raw = vec![10u8, 20, 30, 40, 255];

        write_worker_frame(&mut client, &req, &raw).await.unwrap();
        let (decoded_json, decoded_bin) = read_worker_frame(&mut server).await.unwrap();

        assert_eq!(decoded_json["cmd"], "ocr");
        assert_eq!(decoded_json["sequence"], 7);
        assert_eq!(decoded_bin, raw);
    }

    #[tokio::test]
    async fn worker_frame_rejects_oversized_binary_payload() {
        let (mut client, mut server) = tokio::io::duplex(256);
        let mut bad_prefix = [0u8; 8];
        bad_prefix[0..4].copy_from_slice(&16u32.to_le_bytes());
        bad_prefix[4..8].copy_from_slice(&((MAX_WORKER_BINARY_BYTES as u32) + 1).to_le_bytes());
        client.write_all(&bad_prefix).await.unwrap();

        let err = read_worker_frame(&mut server).await.unwrap_err();
        assert!(matches!(err, FramingError::InvalidPayloadLength(_)));
    }
}
