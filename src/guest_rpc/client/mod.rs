mod driver;
mod reader;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use tokio::io::AsyncWriteExt;
use tokio::net::UnixStream;
use tokio::sync::{Mutex, mpsc, oneshot, watch};
use uuid::Uuid;

use crate::backend::ShellSession;
use crate::guest_rpc::{GuestRequest, GuestRpcReady, GuestRuntimeStats, encode_bytes};
use crate::guest_transport::{GuestTransportEndpoint, connect};
use crate::protocol::{CommandStream, ExecRequest, ExecutionEvent, ShellEvent, ShellRequest};
use crate::workspace::{FileNode, WorkspaceSnapshot};
use crate::{ReadFileResult, Result, SandboxError};

#[derive(Clone)]
pub struct GuestRpcClient {
    pub(super) inner: Arc<GuestRpcInner>,
}

pub(super) struct GuestRpcInner {
    pub(super) writer: Mutex<tokio::io::WriteHalf<UnixStream>>,
    pub(super) next_request: AtomicU64,
    pub(super) ready_tx: watch::Sender<Option<GuestRpcReady>>,
    pub(super) responses: Mutex<HashMap<String, oneshot::Sender<Result<GuestResponse>>>>,
    pub(super) exec_streams: Mutex<HashMap<Uuid, mpsc::Sender<ExecutionEvent>>>,
    pub(super) shell_streams: Mutex<HashMap<Uuid, mpsc::Sender<ShellEvent>>>,
}

pub(super) enum GuestResponse {
    Ack,
    Pong,
    ShellOpened(Uuid),
    Snapshot(WorkspaceSnapshot),
    RuntimeStats(GuestRuntimeStats),
    Files(Vec<FileNode>),
    File(ReadFileResult),
}

impl GuestRpcClient {
    pub async fn connect(endpoint: &GuestTransportEndpoint, timeout: Duration) -> Result<Self> {
        let deadline = Instant::now() + timeout;
        let mut last_error = None;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(last_error
                    .unwrap_or_else(|| SandboxError::timeout("guest RPC did not become ready")));
            }
            let stream = connect(endpoint, remaining).await?;
            let client = Self::from_stream(stream);
            let attempt_timeout = remaining.min(Duration::from_millis(750));
            match client.wait_ready(attempt_timeout).await {
                Ok(_) => return Ok(client),
                Err(error) => {
                    last_error = Some(error);
                    drop(client);
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            }
        }
    }

    pub async fn ping(&self) -> Result<()> {
        self.send_ack(GuestRequest::Ping {
            request_id: self.next_request_id(),
        })
        .await
    }

    pub async fn exec(&self, request: ExecRequest) -> Result<CommandStream> {
        let exec_id = Uuid::new_v4();
        let (sender, receiver) = mpsc::channel(64);
        self.inner.exec_streams.lock().await.insert(exec_id, sender);
        if let Err(error) = self
            .send_request(GuestRequest::Exec {
                request_id: self.next_request_id(),
                exec_id,
                request,
            })
            .await
        {
            self.inner.exec_streams.lock().await.remove(&exec_id);
            return Err(error);
        }
        Ok(CommandStream::new(receiver))
    }

    pub async fn open_shell(&self, request: ShellRequest) -> Result<ShellSession> {
        let session_id = Uuid::new_v4();
        let request_id = self.next_request_id();
        let (sender, receiver) = mpsc::channel(64);
        self.inner
            .shell_streams
            .lock()
            .await
            .insert(session_id, sender);
        let response = self
            .request_response(GuestRequest::OpenShell {
                request_id,
                session_id,
                request,
            })
            .await;
        match response {
            Ok(GuestResponse::ShellOpened(opened)) if opened == session_id => {
                Ok(ShellSession::new(
                    session_id,
                    receiver,
                    Arc::new(driver::RpcShellDriver {
                        client: self.clone(),
                    }),
                ))
            }
            Ok(_) => {
                self.inner.shell_streams.lock().await.remove(&session_id);
                Err(SandboxError::protocol(
                    "guest returned an unexpected shell response",
                ))
            }
            Err(error) => {
                self.inner.shell_streams.lock().await.remove(&session_id);
                Err(error)
            }
        }
    }

    pub async fn snapshot_workspace(&self) -> Result<WorkspaceSnapshot> {
        match self
            .request_response(GuestRequest::SnapshotWorkspace {
                request_id: self.next_request_id(),
            })
            .await?
        {
            GuestResponse::Snapshot(snapshot) => Ok(snapshot),
            _ => Err(SandboxError::protocol(
                "unexpected workspace snapshot response",
            )),
        }
    }

    pub async fn sync_workspace(&self) -> Result<()> {
        self.send_ack(GuestRequest::SyncWorkspace {
            request_id: self.next_request_id(),
        })
        .await
    }

    pub async fn runtime_stats(&self) -> Result<GuestRuntimeStats> {
        match self
            .request_response(GuestRequest::RuntimeStats {
                request_id: self.next_request_id(),
            })
            .await?
        {
            GuestResponse::RuntimeStats(stats) => Ok(stats),
            _ => Err(SandboxError::protocol("unexpected runtime stats response")),
        }
    }

    pub async fn list_files(&self, path: &str) -> Result<Vec<FileNode>> {
        match self
            .request_response(GuestRequest::ListFiles {
                request_id: self.next_request_id(),
                path: path.into(),
            })
            .await?
        {
            GuestResponse::Files(entries) => Ok(entries),
            _ => Err(SandboxError::protocol("unexpected file list response")),
        }
    }

    pub async fn read_file(&self, path: &str, limit: usize) -> Result<ReadFileResult> {
        match self
            .request_response(GuestRequest::ReadFile {
                request_id: self.next_request_id(),
                path: path.into(),
                limit,
            })
            .await?
        {
            GuestResponse::File(file) => Ok(file),
            _ => Err(SandboxError::protocol("unexpected file read response")),
        }
    }

    pub async fn write_file(&self, path: &str, data: &[u8], create_parents: bool) -> Result<()> {
        self.send_ack(GuestRequest::WriteFile {
            request_id: self.next_request_id(),
            path: path.into(),
            data: encode_bytes(data),
            create_parents,
        })
        .await
    }

    pub async fn make_dir(&self, path: &str, recursive: bool) -> Result<()> {
        self.send_ack(GuestRequest::MakeDir {
            request_id: self.next_request_id(),
            path: path.into(),
            recursive,
        })
        .await
    }

    pub async fn remove_path(&self, path: &str, recursive: bool) -> Result<()> {
        self.send_ack(GuestRequest::RemovePath {
            request_id: self.next_request_id(),
            path: path.into(),
            recursive,
        })
        .await
    }

    pub async fn shutdown(&self) -> Result<()> {
        self.send_ack(GuestRequest::Shutdown {
            request_id: self.next_request_id(),
        })
        .await
    }

    pub fn next_request_id(&self) -> String {
        self.inner
            .next_request
            .fetch_add(1, Ordering::Relaxed)
            .to_string()
    }

    pub(super) async fn send_ack(&self, request: GuestRequest) -> Result<()> {
        match self.request_response(request).await? {
            GuestResponse::Ack | GuestResponse::Pong => Ok(()),
            _ => Err(SandboxError::protocol("unexpected guest acknowledgement")),
        }
    }

    async fn request_response(&self, request: GuestRequest) -> Result<GuestResponse> {
        let request_id = request_id(&request).to_string();
        let (sender, receiver) = oneshot::channel();
        self.inner
            .responses
            .lock()
            .await
            .insert(request_id.clone(), sender);
        if let Err(error) = self.send_request(request).await {
            self.inner.responses.lock().await.remove(&request_id);
            return Err(error);
        }
        receiver
            .await
            .map_err(|_| SandboxError::protocol("guest response channel closed"))?
    }

    async fn send_request(&self, request: GuestRequest) -> Result<()> {
        let payload = serde_json::to_vec(&request)
            .map_err(|error| SandboxError::json("encoding guest request", error))?;
        let mut writer = self.inner.writer.lock().await;
        writer
            .write_all(&payload)
            .await
            .map_err(|error| SandboxError::io("writing guest request", error))?;
        writer
            .write_all(b"\n")
            .await
            .map_err(|error| SandboxError::io("terminating guest request", error))
    }

    async fn wait_ready(&self, timeout: Duration) -> Result<GuestRpcReady> {
        if let Some(ready) = self.inner.ready_tx.borrow().clone() {
            return Ok(ready);
        }
        let mut receiver = self.inner.ready_tx.subscribe();
        tokio::time::timeout(timeout, async move {
            loop {
                receiver
                    .changed()
                    .await
                    .map_err(|_| SandboxError::protocol("guest RPC closed before ready"))?;
                if let Some(ready) = receiver.borrow().clone() {
                    return Ok(ready);
                }
            }
        })
        .await
        .map_err(|_| SandboxError::timeout("guest RPC did not become ready"))?
    }
}

impl GuestRpcClient {
    fn from_stream(stream: UnixStream) -> Self {
        let (reader, writer) = tokio::io::split(stream);
        let (ready_tx, _) = watch::channel(None);
        let client = Self {
            inner: Arc::new(GuestRpcInner {
                writer: Mutex::new(writer),
                next_request: AtomicU64::new(1),
                ready_tx,
                responses: Mutex::new(HashMap::new()),
                exec_streams: Mutex::new(HashMap::new()),
                shell_streams: Mutex::new(HashMap::new()),
            }),
        };
        tokio::spawn(client.clone().read_loop(reader));
        client
    }
}

pub(super) fn request_id(request: &GuestRequest) -> &str {
    match request {
        GuestRequest::Ping { request_id }
        | GuestRequest::SnapshotWorkspace { request_id }
        | GuestRequest::SyncWorkspace { request_id }
        | GuestRequest::RuntimeStats { request_id }
        | GuestRequest::ListFiles { request_id, .. }
        | GuestRequest::ReadFile { request_id, .. }
        | GuestRequest::WriteFile { request_id, .. }
        | GuestRequest::MakeDir { request_id, .. }
        | GuestRequest::RemovePath { request_id, .. }
        | GuestRequest::Shutdown { request_id }
        | GuestRequest::Exec { request_id, .. }
        | GuestRequest::OpenShell { request_id, .. }
        | GuestRequest::ShellInput { request_id, .. }
        | GuestRequest::ResizeShell { request_id, .. }
        | GuestRequest::CloseShell { request_id, .. } => request_id,
    }
}
