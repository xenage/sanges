use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use sagens_host::backend::ShellSession;
use sagens_host::boxes::{BoxManager, BoxStatus, LocalBoxService};
use sagens_host::config::{SandboxPolicy, SandboxSpec, WorkspaceConfig};
use sagens_host::guest_rpc::GuestRuntimeStats;
use sagens_host::protocol::{CommandStream, ExecRequest, ShellRequest};
use sagens_host::runtime::{
    SandboxService, SandboxSessionRecord, SandboxSessionState, SandboxSessionSummary,
};
use sagens_host::workspace::{FileNode, ReadFileResult, WorkspaceChange, WorkspaceCommitRecord};
use sagens_host::{Result, SandboxError};
use tempfile::tempdir;
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(Default)]
struct MockRuntime {
    active: Mutex<HashMap<Uuid, String>>,
}

impl MockRuntime {
    async fn expire(&self, sandbox_id: Uuid) {
        self.active.lock().await.remove(&sandbox_id);
    }
}

#[async_trait]
impl SandboxService for MockRuntime {
    async fn list_sandboxes(&self, _: bool) -> Result<Vec<SandboxSessionSummary>> {
        Ok(Vec::new())
    }

    async fn create_sandbox(&self, spec: SandboxSpec) -> Result<SandboxSessionSummary> {
        let sandbox_id = Uuid::new_v4();
        self.active
            .lock()
            .await
            .insert(sandbox_id, spec.workspace_id.clone());
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
        let Some(workspace_id) = self.active.lock().await.remove(&sandbox_id) else {
            return Err(SandboxError::invalid(format!(
                "unknown active sandbox {sandbox_id}"
            )));
        };
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

    async fn list_changes(&self, _: Uuid) -> Result<Vec<WorkspaceChange>> {
        Ok(Vec::new())
    }

    async fn list_files(&self, _: Uuid, _: &str) -> Result<Vec<FileNode>> {
        Ok(Vec::new())
    }

    async fn read_file(&self, _: Uuid, _: &str, _: usize) -> Result<ReadFileResult> {
        Err(SandboxError::backend("read_file is not used in this test"))
    }

    async fn write_file(&self, _: Uuid, _: &str, _: &[u8], _: bool) -> Result<()> {
        Err(SandboxError::backend("write_file is not used in this test"))
    }

    async fn make_dir(&self, _: Uuid, _: &str, _: bool) -> Result<()> {
        Err(SandboxError::backend("make_dir is not used in this test"))
    }

    async fn remove_path(&self, _: Uuid, _: &str, _: bool) -> Result<()> {
        Err(SandboxError::backend(
            "remove_path is not used in this test",
        ))
    }

    async fn exec(&self, _: Uuid, _: ExecRequest) -> Result<CommandStream> {
        Err(SandboxError::backend("exec is not used in this test"))
    }

    async fn open_shell(&self, _: Uuid, _: ShellRequest) -> Result<ShellSession> {
        Err(SandboxError::backend("open_shell is not used in this test"))
    }

    async fn runtime_stats(&self, sandbox_id: Uuid) -> Result<GuestRuntimeStats> {
        if self.active.lock().await.contains_key(&sandbox_id) {
            Ok(GuestRuntimeStats {
                cpu_millicores: 125,
                memory_used_mib: 64,
                fs_used_mib: 32,
                process_count: 2,
            })
        } else {
            Err(SandboxError::invalid(format!(
                "unknown active sandbox {sandbox_id}"
            )))
        }
    }

    async fn sync_workspace(&self, _: Uuid) -> Result<()> {
        Ok(())
    }

    async fn capture_workspace_checkpoint(&self, _: Uuid) -> Result<WorkspaceCommitRecord> {
        Err(SandboxError::backend(
            "capture_workspace_checkpoint is not used in this test",
        ))
    }

    async fn touch_session(&self, sandbox_id: Uuid) -> Result<()> {
        if self.active.lock().await.contains_key(&sandbox_id) {
            Ok(())
        } else {
            Err(SandboxError::invalid(format!(
                "unknown active sandbox {sandbox_id}"
            )))
        }
    }

    async fn restore_workspace_checkpoint(
        &self,
        _: &str,
        _: &str,
    ) -> Result<WorkspaceCommitRecord> {
        Err(SandboxError::backend(
            "restore_workspace_checkpoint is not used in this test",
        ))
    }
}

#[tokio::test]
async fn shutdown_daemon_marks_boxes_stopped_when_runtime_state_is_stale() {
    let temp = tempdir().expect("tempdir");
    let runtime = Arc::new(MockRuntime::default());
    let service = LocalBoxService::new(
        temp.path(),
        WorkspaceConfig { disk_size_mib: 64 },
        SandboxPolicy::default(),
        sagens_host::config::IsolationMode::Compat,
        runtime.clone(),
    )
    .await
    .expect("service");
    let record = service.create_box().await.expect("create");
    let running = service.start_box(record.box_id).await.expect("start");
    let sandbox_id = running.active_sandbox_id.expect("sandbox id");

    runtime.expire(sandbox_id).await;
    service.shutdown_daemon().await.expect("shutdown");

    let restored = service
        .list_boxes()
        .await
        .expect("list")
        .into_iter()
        .find(|record| record.box_id == running.box_id)
        .expect("record");
    assert_eq!(restored.status, BoxStatus::Stopped);
    assert_eq!(restored.active_sandbox_id, None);
    assert!(restored.last_stop_at_ms.is_some());
}
