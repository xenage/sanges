use std::sync::Arc;

use tokio::sync::mpsc;
use uuid::Uuid;

use super::AgentSandboxService;
use crate::Result;
use crate::backend::{ShellDriver, ShellHandle, ShellSession};
use crate::protocol::{CommandStream, ExecRequest, ShellRequest};
use crate::runtime::registry::SessionRegistry;
use crate::workspace::{FileNode, ReadFileResult, WorkspaceChange, WorkspaceCommitRecord};

impl AgentSandboxService {
    pub(super) async fn list_changes_inner(
        &self,
        sandbox_id: Uuid,
    ) -> Result<Vec<WorkspaceChange>> {
        self.registry.note_activity(sandbox_id).await?;
        if let Some(record) = self.registry.history_record(sandbox_id).await {
            return Ok(record.changes);
        }
        let session = self.session(sandbox_id).await?;
        Ok(session.baseline.diff(&session.snapshot_for_diff().await?))
    }

    pub(super) async fn list_files_inner(
        &self,
        sandbox_id: Uuid,
        path: &str,
    ) -> Result<Vec<FileNode>> {
        self.registry.note_activity(sandbox_id).await?;
        self.session(sandbox_id).await?.guest.list_files(path).await
    }

    pub(super) async fn read_file_inner(
        &self,
        sandbox_id: Uuid,
        path: &str,
        limit: usize,
    ) -> Result<ReadFileResult> {
        self.registry.note_activity(sandbox_id).await?;
        self.session(sandbox_id)
            .await?
            .guest
            .read_file(path, limit)
            .await
    }

    pub(super) async fn write_file_inner(
        &self,
        sandbox_id: Uuid,
        path: &str,
        data: &[u8],
        create_parents: bool,
    ) -> Result<()> {
        self.registry.note_activity(sandbox_id).await?;
        self.session(sandbox_id)
            .await?
            .guest
            .write_file(path, data, create_parents)
            .await
    }

    pub(super) async fn make_dir_inner(
        &self,
        sandbox_id: Uuid,
        path: &str,
        recursive: bool,
    ) -> Result<()> {
        self.registry.note_activity(sandbox_id).await?;
        self.session(sandbox_id)
            .await?
            .guest
            .make_dir(path, recursive)
            .await
    }

    pub(super) async fn remove_path_inner(
        &self,
        sandbox_id: Uuid,
        path: &str,
        recursive: bool,
    ) -> Result<()> {
        self.registry.note_activity(sandbox_id).await?;
        self.session(sandbox_id)
            .await?
            .guest
            .remove_path(path, recursive)
            .await
    }

    pub(super) async fn exec_inner(
        &self,
        sandbox_id: Uuid,
        mut request: ExecRequest,
    ) -> Result<CommandStream> {
        self.registry.note_activity(sandbox_id).await?;
        let session = self.session(sandbox_id).await?;
        let permit = session.try_acquire_exec()?;
        if request.timeout_ms.is_none() {
            request.timeout_ms = session.summary.read().await.policy.timeout_ms;
        }
        let stream = session.guest.exec(request).await?;
        wrap_exec_stream(stream, sandbox_id, self.registry.clone(), permit)
    }

    pub(super) async fn open_shell_inner(
        &self,
        sandbox_id: Uuid,
        request: ShellRequest,
    ) -> Result<ShellSession> {
        self.registry.note_activity(sandbox_id).await?;
        let session = self
            .session(sandbox_id)
            .await?
            .guest
            .open_shell(request)
            .await?;
        wrap_shell_session(session, sandbox_id, self.registry.clone())
    }

    pub(super) async fn sync_workspace_inner(&self, sandbox_id: Uuid) -> Result<()> {
        self.registry.note_activity(sandbox_id).await?;
        self.session(sandbox_id).await?.guest.sync_workspace().await
    }

    pub(super) async fn capture_workspace_checkpoint_inner(
        &self,
        sandbox_id: Uuid,
    ) -> Result<WorkspaceCommitRecord> {
        self.registry.note_activity(sandbox_id).await?;
        let session = self.session(sandbox_id).await?;
        session.guest.sync_workspace().await?;
        let current = session.snapshot_for_diff().await?;
        session.guest.sync_workspace().await?;
        self.workspace
            .create_internal_checkpoint(&session.workspace, &session.baseline, &current)
            .await
    }
}

fn wrap_exec_stream(
    mut stream: CommandStream,
    sandbox_id: Uuid,
    registry: SessionRegistry,
    permit: tokio::sync::OwnedSemaphorePermit,
) -> Result<CommandStream> {
    let (tx, rx) = mpsc::channel(64);
    tokio::spawn(async move {
        while let Some(event) = stream.next().await {
            let _ = registry.note_activity(sandbox_id).await;
            let terminal = matches!(event, crate::protocol::ExecutionEvent::Exit { .. });
            if tx.send(event).await.is_err() || terminal {
                break;
            }
        }
    });
    Ok(CommandStream::new(rx).with_exec_permit(permit))
}

fn wrap_shell_session(
    session: ShellSession,
    sandbox_id: Uuid,
    registry: SessionRegistry,
) -> Result<ShellSession> {
    let (handle, mut events) = session.into_parts();
    let (tx, rx) = mpsc::channel(32);
    let output_registry = registry.clone();
    tokio::spawn(async move {
        while let Some(event) = events.recv().await {
            let _ = output_registry.note_activity(sandbox_id).await;
            let terminal = matches!(event, crate::protocol::ShellEvent::Exit(_));
            if tx.send(event).await.is_err() || terminal {
                break;
            }
        }
    });
    Ok(ShellSession::new(
        handle.id(),
        rx,
        Arc::new(ActivityShellDriver {
            sandbox_id,
            registry,
            inner: handle,
        }),
    ))
}

struct ActivityShellDriver {
    sandbox_id: Uuid,
    registry: SessionRegistry,
    inner: ShellHandle,
}

#[async_trait::async_trait]
impl ShellDriver for ActivityShellDriver {
    async fn send_input(&self, _: Uuid, data: Vec<u8>) -> Result<()> {
        self.registry.note_activity(self.sandbox_id).await?;
        self.inner.send_input(data).await
    }

    async fn resize(&self, _: Uuid, cols: u16, rows: u16) -> Result<()> {
        self.registry.note_activity(self.sandbox_id).await?;
        self.inner.resize(cols, rows).await
    }

    async fn close(&self, _: Uuid) -> Result<()> {
        self.registry.note_activity(self.sandbox_id).await?;
        self.inner.close().await
    }
}
