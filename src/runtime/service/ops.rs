use std::collections::BTreeMap;

use tokio::sync::mpsc;
use uuid::Uuid;

use super::AgentSandboxService;
use crate::Result;
use crate::backend::ShellSession;
use crate::protocol::{CommandStream, ExecRequest, ShellRequest};
use crate::workspace::{FileNode, ReadFileResult, WorkspaceCommitRecord};

impl AgentSandboxService {
    pub(super) async fn list_files_inner(
        &self,
        sandbox_id: Uuid,
        path: &str,
    ) -> Result<Vec<FileNode>> {
        self.session(sandbox_id).await?.guest.list_files(path).await
    }

    pub(super) async fn read_file_inner(
        &self,
        sandbox_id: Uuid,
        path: &str,
        limit: usize,
    ) -> Result<ReadFileResult> {
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
        let session = self.session(sandbox_id).await?;
        let permit = session.try_acquire_exec()?;
        if request.timeout_ms.is_none() {
            request.timeout_ms = session.summary.read().await.policy.timeout_ms;
        }
        let stream = session.guest.exec(request).await?;
        wrap_exec_stream(stream, permit)
    }

    pub(super) async fn open_shell_inner(
        &self,
        sandbox_id: Uuid,
        request: ShellRequest,
    ) -> Result<ShellSession> {
        self.session(sandbox_id)
            .await?
            .guest
            .open_shell(request)
            .await
    }

    pub(super) async fn sync_workspace_inner(&self, sandbox_id: Uuid) -> Result<()> {
        self.session(sandbox_id).await?.guest.sync_workspace().await
    }

    pub(super) async fn capture_workspace_checkpoint_inner(
        &self,
        sandbox_id: Uuid,
        name: Option<String>,
        metadata: BTreeMap<String, String>,
    ) -> Result<WorkspaceCommitRecord> {
        let session = self.session(sandbox_id).await?;
        session.guest.sync_workspace().await?;
        let current = session.snapshot_for_diff().await?;
        session.guest.sync_workspace().await?;
        self.workspace
            .create_internal_checkpoint(
                &session.workspace,
                &session.baseline,
                &current,
                name,
                metadata,
            )
            .await
    }
}

fn wrap_exec_stream(
    mut stream: CommandStream,
    permit: tokio::sync::OwnedSemaphorePermit,
) -> Result<CommandStream> {
    let (tx, rx) = mpsc::channel(64);
    tokio::spawn(async move {
        while let Some(event) = stream.next().await {
            let terminal = matches!(event, crate::protocol::ExecutionEvent::Exit { .. });
            if tx.send(event).await.is_err() || terminal {
                break;
            }
        }
    });
    Ok(CommandStream::new(rx).with_exec_permit(permit))
}
