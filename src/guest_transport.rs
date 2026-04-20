use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use tokio::net::UnixStream;

use crate::{Result, SandboxError};

pub const DEFAULT_GUEST_RPC_PORT: u32 = 11000;

#[derive(Debug, Clone)]
pub struct GuestTransportEndpoint {
    pub socket_path: PathBuf,
    pub port: u32,
}

impl GuestTransportEndpoint {
    pub fn new(socket_path: PathBuf, port: u32) -> Self {
        Self { socket_path, port }
    }
}

pub async fn connect(endpoint: &GuestTransportEndpoint, timeout: Duration) -> Result<UnixStream> {
    let _port = endpoint.port;
    wait_for_socket(&endpoint.socket_path, timeout).await?;
    UnixStream::connect(&endpoint.socket_path)
        .await
        .map_err(|error| SandboxError::io("connecting to guest vsock bridge", error))
}

async fn wait_for_socket(path: &Path, timeout: Duration) -> Result<()> {
    let started = Instant::now();
    loop {
        if tokio::fs::try_exists(path)
            .await
            .map_err(|error| SandboxError::io("checking guest vsock bridge socket", error))?
        {
            return Ok(());
        }
        if started.elapsed() > timeout {
            return Err(SandboxError::timeout(format!(
                "timed out waiting for guest vsock bridge {}",
                path.display()
            )));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
