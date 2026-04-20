use std::sync::Arc;

use tokio::io::{AsyncWriteExt, BufReader};
use tokio::sync::Mutex;
use tokio_vsock::VsockStream;

use crate::guest_rpc::{GuestEvent, GuestRequest};
use crate::{Result, SandboxError};

pub type GuestWriter = Arc<Mutex<tokio::io::WriteHalf<VsockStream>>>;
pub type GuestLines = tokio::io::Lines<BufReader<tokio::io::ReadHalf<VsockStream>>>;

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

pub async fn next_request(lines: &mut GuestLines) -> Result<Option<GuestRequest>> {
    match lines.next_line().await {
        Ok(Some(line)) => serde_json::from_str(&line)
            .map(Some)
            .map_err(|error| SandboxError::json("decoding guest request", error)),
        Ok(None) => Ok(None),
        Err(error) => Err(SandboxError::io("reading guest request", error)),
    }
}
