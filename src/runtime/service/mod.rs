mod lifecycle;
mod ops;

use std::sync::Arc;

use async_trait::async_trait;
use uuid::Uuid;

use crate::Result;
use crate::backend::libkrun::LibkrunBackend;
use crate::backend::{Backend, ShellSession};
use crate::bundle;
use crate::config::{RuntimeConfig, SandboxSpec};
use crate::guest_rpc::GuestRuntimeStats;
use crate::host_hardening;
use crate::host_log;
use crate::protocol::{CommandStream, ExecRequest, ShellRequest};
use crate::runtime::registry::{ManagedSandbox, SessionRegistry};
use crate::runtime::{SandboxSessionRecord, SandboxSessionSummary};
use crate::workspace::{
    FileNode, ReadFileResult, WorkspaceChange, WorkspaceCommitRecord, WorkspaceStore,
};

#[async_trait]
pub trait SandboxService: Send + Sync {
    async fn list_sandboxes(&self, include_history: bool) -> Result<Vec<SandboxSessionSummary>>;
    async fn create_sandbox(&self, spec: SandboxSpec) -> Result<SandboxSessionSummary>;
    async fn destroy_sandbox(&self, sandbox_id: Uuid) -> Result<SandboxSessionRecord>;
    async fn list_changes(&self, sandbox_id: Uuid) -> Result<Vec<WorkspaceChange>>;
    async fn list_files(&self, sandbox_id: Uuid, path: &str) -> Result<Vec<FileNode>>;
    async fn read_file(&self, sandbox_id: Uuid, path: &str, limit: usize)
    -> Result<ReadFileResult>;
    async fn write_file(
        &self,
        sandbox_id: Uuid,
        path: &str,
        data: &[u8],
        create_parents: bool,
    ) -> Result<()>;
    async fn make_dir(&self, sandbox_id: Uuid, path: &str, recursive: bool) -> Result<()>;
    async fn remove_path(&self, sandbox_id: Uuid, path: &str, recursive: bool) -> Result<()>;
    async fn exec(&self, sandbox_id: Uuid, request: ExecRequest) -> Result<CommandStream>;
    async fn open_shell(&self, sandbox_id: Uuid, request: ShellRequest) -> Result<ShellSession>;
    async fn runtime_stats(&self, sandbox_id: Uuid) -> Result<GuestRuntimeStats>;
    async fn sync_workspace(&self, sandbox_id: Uuid) -> Result<()>;
    async fn capture_workspace_checkpoint(&self, sandbox_id: Uuid)
    -> Result<WorkspaceCommitRecord>;
    async fn touch_session(&self, sandbox_id: Uuid) -> Result<()>;
    async fn restore_workspace_checkpoint(
        &self,
        workspace_id: &str,
        checkpoint_id: &str,
    ) -> Result<WorkspaceCommitRecord>;
}

pub struct AgentSandboxService {
    config: RuntimeConfig,
    backend: Arc<dyn Backend>,
    workspace: WorkspaceStore,
    registry: SessionRegistry,
}

impl AgentSandboxService {
    pub async fn new(config: RuntimeConfig) -> Result<Self> {
        Self::with_backend(config, Arc::new(LibkrunBackend)).await
    }

    pub async fn with_backend(config: RuntimeConfig, backend: Arc<dyn Backend>) -> Result<Self> {
        let config = resolve_config(config).await?;
        let workspace = WorkspaceStore::new(config.state_dir.clone(), config.workspace.clone());
        workspace.ensure_layout().await?;
        let service = Self {
            config,
            backend,
            workspace,
            registry: SessionRegistry::new(),
        };
        service.spawn_idle_reaper();
        Ok(service)
    }

    async fn session(&self, sandbox_id: Uuid) -> Result<Arc<ManagedSandbox>> {
        self.registry.active(sandbox_id).await
    }

    fn spawn_idle_reaper(&self) {
        let registry = self.registry.clone();
        let idle_timeout = self.config.lifecycle.idle_timeout;
        let reap_interval = self.config.lifecycle.reap_interval;
        let service = self.clone_for_reaper();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(reap_interval).await;
                for sandbox_id in registry.idle_candidates(idle_timeout).await {
                    if let Err(error) = service.destroy_sandbox_inner(sandbox_id).await {
                        host_log::emit(
                            "runtime",
                            format!("idle reap failed sandbox_id={sandbox_id} error={error}"),
                        );
                    }
                }
            }
        });
    }

    fn clone_for_reaper(&self) -> ReaperHandle {
        ReaperHandle {
            workspace: self.workspace.clone(),
            registry: self.registry.clone(),
            shutdown_grace: self.config.lifecycle.shutdown_grace,
        }
    }
}

#[async_trait]
impl SandboxService for AgentSandboxService {
    async fn list_sandboxes(&self, include_history: bool) -> Result<Vec<SandboxSessionSummary>> {
        Ok(self.registry.list(include_history).await)
    }

    async fn create_sandbox(&self, spec: SandboxSpec) -> Result<SandboxSessionSummary> {
        self.create_sandbox_inner(spec).await
    }

    async fn destroy_sandbox(&self, sandbox_id: Uuid) -> Result<SandboxSessionRecord> {
        self.destroy_sandbox_inner(sandbox_id).await
    }

    async fn list_changes(&self, sandbox_id: Uuid) -> Result<Vec<WorkspaceChange>> {
        self.list_changes_inner(sandbox_id).await
    }

    async fn list_files(&self, sandbox_id: Uuid, path: &str) -> Result<Vec<FileNode>> {
        self.list_files_inner(sandbox_id, path).await
    }

    async fn read_file(
        &self,
        sandbox_id: Uuid,
        path: &str,
        limit: usize,
    ) -> Result<ReadFileResult> {
        self.read_file_inner(sandbox_id, path, limit).await
    }

    async fn write_file(
        &self,
        sandbox_id: Uuid,
        path: &str,
        data: &[u8],
        create_parents: bool,
    ) -> Result<()> {
        self.write_file_inner(sandbox_id, path, data, create_parents)
            .await
    }

    async fn make_dir(&self, sandbox_id: Uuid, path: &str, recursive: bool) -> Result<()> {
        self.make_dir_inner(sandbox_id, path, recursive).await
    }

    async fn remove_path(&self, sandbox_id: Uuid, path: &str, recursive: bool) -> Result<()> {
        self.remove_path_inner(sandbox_id, path, recursive).await
    }

    async fn exec(&self, sandbox_id: Uuid, request: ExecRequest) -> Result<CommandStream> {
        self.exec_inner(sandbox_id, request).await
    }

    async fn open_shell(&self, sandbox_id: Uuid, request: ShellRequest) -> Result<ShellSession> {
        self.open_shell_inner(sandbox_id, request).await
    }

    async fn runtime_stats(&self, sandbox_id: Uuid) -> Result<GuestRuntimeStats> {
        let session = self.session(sandbox_id).await?;
        session.guest.runtime_stats().await
    }

    async fn sync_workspace(&self, sandbox_id: Uuid) -> Result<()> {
        self.sync_workspace_inner(sandbox_id).await
    }

    async fn capture_workspace_checkpoint(
        &self,
        sandbox_id: Uuid,
    ) -> Result<WorkspaceCommitRecord> {
        self.capture_workspace_checkpoint_inner(sandbox_id).await
    }

    async fn touch_session(&self, sandbox_id: Uuid) -> Result<()> {
        self.registry.note_activity(sandbox_id).await
    }

    async fn restore_workspace_checkpoint(
        &self,
        workspace_id: &str,
        checkpoint_id: &str,
    ) -> Result<WorkspaceCommitRecord> {
        self.restore_workspace_checkpoint_inner(workspace_id, checkpoint_id)
            .await
    }
}

async fn resolve_config(mut config: RuntimeConfig) -> Result<RuntimeConfig> {
    config.guest = bundle::resolve_guest_paths(
        &config.state_dir,
        &config.artifact_bundle.bundle_id,
        &config.guest,
    )
    .await?;
    config.validate()?;
    let hardening_status = host_hardening::preflight_runtime(&config).await?;
    for warning in hardening_status.warnings {
        eprintln!("sagens hardening warning: {warning}");
    }
    Ok(config)
}

#[derive(Clone)]
struct ReaperHandle {
    workspace: WorkspaceStore,
    registry: SessionRegistry,
    shutdown_grace: std::time::Duration,
}

impl ReaperHandle {
    async fn destroy_sandbox_inner(&self, sandbox_id: Uuid) -> Result<SandboxSessionRecord> {
        if let Some(record) = self.registry.history_record(sandbox_id).await {
            return Ok(record);
        }
        let session = self.registry.remove_active(sandbox_id).await?;
        let _ = session.guest.sync_workspace().await;
        let final_snapshot = session.snapshot_for_diff().await?;
        let changes = session.baseline.diff(&final_snapshot);
        let mut summary = session.summary.write().await;
        summary.state = crate::runtime::SandboxSessionState::Destroyed;
        summary.ended_at_ms = Some(now_ms());
        let record = SandboxSessionRecord {
            summary: summary.clone(),
            changes,
        };
        drop(summary);
        let _ = session.guest.sync_workspace().await;
        let _ = session.guest.shutdown().await;
        let shutdown = session.backend.shutdown();
        let _ = tokio::time::timeout(self.shutdown_grace, shutdown).await;
        self.workspace.destroy_run(&session.run_layout).await?;
        self.registry.save_history(record.clone()).await;
        Ok(record)
    }
}

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};

    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_millis() as u64,
        Err(_) => 0,
    }
}
