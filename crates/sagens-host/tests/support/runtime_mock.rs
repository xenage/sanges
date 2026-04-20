use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use async_trait::async_trait;
use sagens_host::backend::ShellSession;
use sagens_host::config::{SandboxPolicy, SandboxSpec};
use sagens_host::guest_rpc::GuestRuntimeStats;
use sagens_host::protocol::{
    CommandStream, ExecExit, ExecRequest, ExecutionEvent, OutputStream, ShellRequest,
};
use sagens_host::runtime::{
    SandboxService, SandboxSessionRecord, SandboxSessionState, SandboxSessionSummary,
};
use sagens_host::workspace::{
    FileKind, FileNode, ReadFileResult, WorkspaceChange, WorkspaceChangeKind,
    WorkspaceCommitRecord, WorkspaceCommitSummary,
};
use sagens_host::{Result, SandboxError};
use tokio::sync::{Mutex, mpsc};
use uuid::Uuid;

struct MockSession {
    workspace_id: String,
}

#[derive(Default)]
struct MockState {
    active: HashMap<Uuid, MockSession>,
    workspace_files: HashMap<String, BTreeMap<String, Vec<u8>>>,
    create_count: usize,
    committed: usize,
    checkpoint_heads: HashMap<String, String>,
}

#[derive(Default)]
pub struct MockSandboxService {
    state: Mutex<MockState>,
}

impl MockSandboxService {
    pub async fn create_count(&self) -> usize {
        self.state.lock().await.create_count
    }

    pub async fn active_sandbox(&self, workspace_id: &str) -> Option<Uuid> {
        self.state
            .lock()
            .await
            .active
            .iter()
            .find_map(|(sandbox_id, session)| {
                (session.workspace_id == workspace_id).then_some(*sandbox_id)
            })
    }

    pub async fn expire_workspace(&self, workspace_id: &str) {
        self.state
            .lock()
            .await
            .active
            .retain(|_, session| session.workspace_id != workspace_id);
    }

    async fn session_workspace(&self, sandbox_id: Uuid) -> Result<String> {
        self.state
            .lock()
            .await
            .active
            .get(&sandbox_id)
            .map(|session| session.workspace_id.clone())
            .ok_or_else(|| SandboxError::invalid(format!("unknown active sandbox {sandbox_id}")))
    }
}

#[async_trait]
impl SandboxService for MockSandboxService {
    async fn list_sandboxes(&self, _: bool) -> Result<Vec<SandboxSessionSummary>> {
        Ok(Vec::new())
    }

    async fn create_sandbox(&self, spec: SandboxSpec) -> Result<SandboxSessionSummary> {
        let sandbox_id = Uuid::new_v4();
        let mut state = self.state.lock().await;
        state.create_count += 1;
        state.active.insert(
            sandbox_id,
            MockSession {
                workspace_id: spec.workspace_id.clone(),
            },
        );
        Ok(SandboxSessionSummary {
            sandbox_id,
            workspace_id: spec.workspace_id,
            state: SandboxSessionState::Active,
            policy: spec.policy,
            started_at_ms: 1,
            ended_at_ms: None,
        })
    }

    async fn destroy_sandbox(&self, sandbox_id: Uuid) -> Result<SandboxSessionRecord> {
        let workspace_id = self.session_workspace(sandbox_id).await?;
        self.state.lock().await.active.remove(&sandbox_id);
        Ok(SandboxSessionRecord {
            summary: SandboxSessionSummary {
                sandbox_id,
                workspace_id,
                state: SandboxSessionState::Destroyed,
                policy: SandboxPolicy::default(),
                started_at_ms: 1,
                ended_at_ms: Some(2),
            },
            changes: Vec::new(),
        })
    }

    async fn list_changes(&self, sandbox_id: Uuid) -> Result<Vec<WorkspaceChange>> {
        let workspace_id = self.session_workspace(sandbox_id).await?;
        let files = self
            .state
            .lock()
            .await
            .workspace_files
            .get(&workspace_id)
            .cloned()
            .unwrap_or_default();
        Ok(files
            .into_keys()
            .map(|path| WorkspaceChange {
                path,
                kind: WorkspaceChangeKind::Added,
                kind_after: Some(FileKind::File),
            })
            .collect())
    }

    async fn list_files(&self, sandbox_id: Uuid, _: &str) -> Result<Vec<FileNode>> {
        let workspace_id = self.session_workspace(sandbox_id).await?;
        let files = self
            .state
            .lock()
            .await
            .workspace_files
            .get(&workspace_id)
            .cloned()
            .unwrap_or_default();
        Ok(files
            .into_iter()
            .map(|(path, data)| FileNode {
                path,
                kind: FileKind::File,
                size: data.len() as u64,
                digest: None,
                target: None,
            })
            .collect())
    }

    async fn read_file(&self, sandbox_id: Uuid, path: &str, _: usize) -> Result<ReadFileResult> {
        let workspace_id = self.session_workspace(sandbox_id).await?;
        let data = self
            .state
            .lock()
            .await
            .workspace_files
            .get(&workspace_id)
            .and_then(|files| files.get(path).cloned())
            .unwrap_or_default();
        Ok(ReadFileResult {
            path: path.into(),
            data,
            truncated: false,
        })
    }

    async fn write_file(&self, sandbox_id: Uuid, path: &str, data: &[u8], _: bool) -> Result<()> {
        let workspace_id = self.session_workspace(sandbox_id).await?;
        self.state
            .lock()
            .await
            .workspace_files
            .entry(workspace_id)
            .or_default()
            .insert(path.into(), data.to_vec());
        Ok(())
    }

    async fn make_dir(&self, _: Uuid, _: &str, _: bool) -> Result<()> {
        Ok(())
    }

    async fn remove_path(&self, sandbox_id: Uuid, path: &str, _: bool) -> Result<()> {
        let workspace_id = self.session_workspace(sandbox_id).await?;
        if let Some(files) = self
            .state
            .lock()
            .await
            .workspace_files
            .get_mut(&workspace_id)
        {
            files.remove(path);
        }
        Ok(())
    }

    async fn exec(&self, sandbox_id: Uuid, _: ExecRequest) -> Result<CommandStream> {
        let workspace_id = self.session_workspace(sandbox_id).await?;
        let (tx, rx) = mpsc::channel(2);
        tokio::spawn(async move {
            let _ = tx
                .send(ExecutionEvent::Output {
                    stream: OutputStream::Stdout,
                    data: format!("{workspace_id}:{sandbox_id}\n").into_bytes(),
                })
                .await;
            let _ = tx
                .send(ExecutionEvent::Exit {
                    status: ExecExit::Success,
                })
                .await;
        });
        Ok(CommandStream::new(rx))
    }

    async fn open_shell(&self, _: Uuid, _: ShellRequest) -> Result<ShellSession> {
        let (_, rx) = mpsc::channel(1);
        Ok(ShellSession::new(
            Uuid::new_v4(),
            rx,
            Arc::new(NoopShellDriver),
        ))
    }

    async fn runtime_stats(&self, sandbox_id: Uuid) -> Result<GuestRuntimeStats> {
        let workspace_id = self.session_workspace(sandbox_id).await?;
        let process_count = self
            .state
            .lock()
            .await
            .workspace_files
            .get(&workspace_id)
            .map(|files| files.len() as u32)
            .unwrap_or(0)
            .saturating_add(1);
        Ok(GuestRuntimeStats {
            cpu_millicores: 125,
            memory_used_mib: 64,
            fs_used_mib: 32,
            process_count,
        })
    }

    async fn sync_workspace(&self, sandbox_id: Uuid) -> Result<()> {
        self.session_workspace(sandbox_id).await.map(|_| ())
    }

    async fn capture_workspace_checkpoint(
        &self,
        sandbox_id: Uuid,
    ) -> Result<WorkspaceCommitRecord> {
        let workspace_id = self.session_workspace(sandbox_id).await?;
        let mut state = self.state.lock().await;
        state.committed += 1;
        let checkpoint_id = format!("commit-{}", state.committed);
        let source_checkpoint_id = state.checkpoint_heads.get(&workspace_id).cloned();
        state
            .checkpoint_heads
            .insert(workspace_id.clone(), checkpoint_id.clone());
        Ok(WorkspaceCommitRecord {
            summary: WorkspaceCommitSummary {
                checkpoint_id,
                workspace_id,
                name: None,
                metadata: std::collections::BTreeMap::new(),
                created_at_ms: 1,
            },
            source_checkpoint_id,
            changes: Vec::new(),
        })
    }

    async fn touch_session(&self, sandbox_id: Uuid) -> Result<()> {
        self.session_workspace(sandbox_id).await.map(|_| ())
    }

    async fn restore_workspace_checkpoint(
        &self,
        _: &str,
        _: &str,
    ) -> Result<WorkspaceCommitRecord> {
        Err(SandboxError::invalid(
            "restore is not used in runtime_lifecycle tests",
        ))
    }
}

struct NoopShellDriver;

#[async_trait]
impl sagens_host::backend::ShellDriver for NoopShellDriver {
    async fn send_input(&self, _: Uuid, _: Vec<u8>) -> Result<()> {
        Ok(())
    }

    async fn resize(&self, _: Uuid, _: u16, _: u16) -> Result<()> {
        Ok(())
    }

    async fn close(&self, _: Uuid) -> Result<()> {
        Ok(())
    }
}
