use std::pin::Pin;
use std::sync::Arc;

use tokio::io::{AsyncBufRead, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::Mutex;

use crate::guest_rpc::{GuestEvent, GuestRequest};
use crate::{Result, SandboxError};

pub type GuestReader = Pin<Box<dyn AsyncRead + Send>>;
pub type GuestWriterStream = Pin<Box<dyn AsyncWrite + Send>>;
pub type GuestWriter = Arc<Mutex<GuestWriterStream>>;
pub type GuestLines = tokio::io::Lines<BufReader<GuestReader>>;

pub async fn send_event(writer: &GuestWriter, event: &GuestEvent) -> Result<()> {
    let payload = serde_json::to_vec(event)
        .map_err(|error| SandboxError::json("encoding guest event", error))?;
    let mut writer = writer.lock().await;
    writer
        .write_all(&payload)
        .await
        .map_err(|error| SandboxError::io("writing guest event", error))?;
    writer
        .write_all(b"\n")
        .await
        .map_err(|error| SandboxError::io("terminating guest event", error))
}

pub fn box_reader<T>(reader: T) -> GuestReader
where
    T: AsyncRead + Send + 'static,
{
    Box::pin(reader)
}

pub fn box_writer<T>(writer: T) -> GuestWriterStream
where
    T: AsyncWrite + Send + 'static,
{
    Box::pin(writer)
}

pub async fn next_request<T>(
    lines: &mut tokio::io::Lines<BufReader<T>>,
) -> Result<Option<GuestRequest>>
where
    T: AsyncBufRead + Unpin,
{
    match lines.next_line().await {
        Ok(Some(line)) => serde_json::from_str(&line)
            .map(Some)
            .map_err(|error| SandboxError::json("decoding guest request", error)),
        Ok(None) => Ok(None),
        Err(error) => Err(SandboxError::io("reading guest request", error)),
    }
}
