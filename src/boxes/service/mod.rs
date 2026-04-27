mod manager;
mod policy;
mod record;

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::config::{IsolationMode, SandboxPolicy};
use crate::host_log;
use crate::protocol::{CommandStream, ExecRequest, ShellRequest};
use crate::runtime::SandboxService;
use crate::workspace::{
    CheckpointRestoreMode, FileNode, ReadFileResult, WorkspaceCheckpointRecord, WorkspaceStore,
};
use crate::{Result, SandboxError, WorkspaceConfig};

use super::helpers::{missing_runtime_session, now_ms};
use super::{BoxRecord, BoxSettingValue, BoxStatus, BoxStore};
use policy::{box_policy, validate_numeric_setting};

pub use manager::BoxManager;

pub struct LocalBoxService {
    pub(super) state_dir: PathBuf,
    pub(super) runtime: Arc<dyn SandboxService>,
    pub(super) boxes: BoxStore,
    pub(super) workspace: WorkspaceStore,
    pub(super) workspace_config: WorkspaceConfig,
    pub(super) default_policy: SandboxPolicy,
    pub(super) isolation_mode: IsolationMode,
    pub(super) active: RwLock<HashMap<Uuid, Uuid>>,
}

#[async_trait]
impl BoxManager for LocalBoxService {
    async fn list_boxes(&self) -> Result<Vec<BoxRecord>> {
        let mut records = Vec::new();
        for record in self.boxes.list().await? {
            let record = self.hydrate_record(record).await?;
            records.push(self.attach_runtime_usage(record).await?);
        }
        Ok(records)
    }

    async fn get_box(&self, box_id: Uuid) -> Result<BoxRecord> {
        self.attach_runtime_usage(self.read_box(box_id).await?)
            .await
    }

    async fn create_box(&self) -> Result<BoxRecord> {
        self.create_named_box(None).await
    }

    async fn create_named_box(&self, name: Option<String>) -> Result<BoxRecord> {
        self.create_box_record(name, None).await
    }

    async fn set_box_setting(&self, box_id: Uuid, setting: BoxSettingValue) -> Result<BoxRecord> {
        let mut record = self.read_box(box_id).await?;
        if record.status == BoxStatus::Running {
            return Err(SandboxError::conflict(format!(
                "BOX {box_id} must be stopped before updating settings"
            )));
        }
        let settings = record.settings.as_mut().ok_or_else(|| {
            SandboxError::backend(format!("BOX {box_id} is missing persisted settings"))
        })?;
        match setting {
            BoxSettingValue::CpuCores { value } => {
                validate_numeric_setting("cpu_cores", value, settings.cpu_cores.max, 1)?;
                settings.cpu_cores.current = value;
            }
            BoxSettingValue::MemoryMb { value } => {
                validate_numeric_setting("memory_mb", value, settings.memory_mb.max, 128)?;
                settings.memory_mb.current = value;
            }
            BoxSettingValue::FsSizeMib { value } => {
                validate_numeric_setting("fs_size_mib", value, settings.fs_size_mib.max, 64)?;
                if value != settings.fs_size_mib.current {
                    self.workspace
                        .resize_workspace(&box_id.to_string(), value)
                        .await?;
                    settings.fs_size_mib.current = value;
                }
            }
            BoxSettingValue::MaxProcesses { value } => {
                validate_numeric_setting("max_processes", value, settings.max_processes.max, 1)?;
                settings.max_processes.current = value;
            }
            BoxSettingValue::NetworkEnabled { value } => {
                if value && !settings.network_enabled.max {
                    return Err(SandboxError::invalid(
                        "network_enabled cannot be set to true for this BOX",
                    ));
                }
                settings.network_enabled.current = value;
            }
        }
        self.boxes.write(&record).await?;
        self.hydrate_record(record).await
    }

    async fn start_box(&self, box_id: Uuid) -> Result<BoxRecord> {
        let mut record = self.read_box(box_id).await?;
        if record.status == BoxStatus::Running {
            return Err(SandboxError::conflict(format!(
                "BOX {box_id} is already running"
            )));
        }
        host_log::emit("box", format!("start requested box_id={box_id}"));
        match self
            .runtime
            .create_sandbox(crate::config::SandboxSpec {
                workspace_id: box_id.to_string(),
                policy: box_policy(&record, self.default_policy.timeout_ms)?,
                restore_commit: None,
            })
            .await
        {
            Ok(session) => {
                self.active.write().await.insert(box_id, session.sandbox_id);
                record.status = BoxStatus::Running;
                record.active_sandbox_id = Some(session.sandbox_id);
                record.last_start_at_ms = Some(now_ms());
                record.last_error = None;
                host_log::emit(
                    "box",
                    format!(
                        "start succeeded box_id={} sandbox_id={}",
                        record.box_id, session.sandbox_id
                    ),
                );
                self.boxes.write(&record).await?;
                self.attach_runtime_usage(record).await
            }
            Err(error) => {
                host_log::emit("box", format!("start failed box_id={box_id} error={error}"));
                self.set_failed(record, error.to_string()).await?;
                Err(error)
            }
        }
    }

    async fn stop_box(&self, box_id: Uuid) -> Result<BoxRecord> {
        let mut record = self.read_box(box_id).await?;
        if record.status != BoxStatus::Running {
            return Err(SandboxError::conflict(format!(
                "BOX {box_id} is not running"
            )));
        }
        host_log::emit("box", format!("stop requested box_id={box_id}"));
        let sandbox_id = self.running_sandbox_id(box_id).await?;
        match self.runtime.destroy_sandbox(sandbox_id).await {
            Ok(_) => {
                self.active.write().await.remove(&box_id);
                record.status = BoxStatus::Stopped;
                record.active_sandbox_id = None;
                record.last_stop_at_ms = Some(now_ms());
                record.last_error = None;
                host_log::emit(
                    "box",
                    format!(
                        "stop succeeded box_id={} sandbox_id={sandbox_id}",
                        record.box_id
                    ),
                );
                self.boxes.write(&record).await?;
                Ok(record)
            }
            Err(error) if missing_runtime_session(&error) => {
                self.mark_stopped(record).await?;
                self.read_box(box_id).await
            }
            Err(error) => {
                host_log::emit(
                    "box",
                    format!(
                        "stop failed box_id={} sandbox_id={} error={error}",
                        record.box_id, sandbox_id
                    ),
                );
                self.set_failed(record, error.to_string()).await?;
                Err(error)
            }
        }
    }

    async fn remove_box(&self, box_id: Uuid) -> Result<()> {
        let mut record = self.read_box(box_id).await?;
        if record.status == BoxStatus::Running {
            self.stop_box(box_id).await?;
            record = self.read_box(box_id).await?;
        }
        record.status = BoxStatus::Removing;
        record.active_sandbox_id = None;
        record.last_error = None;
        self.boxes.write(&record).await?;
        self.workspace.remove_workspace(&box_id.to_string()).await?;
        self.active.write().await.remove(&box_id);
        self.boxes.remove(box_id).await
    }

    async fn list_files(&self, box_id: Uuid, path: &str) -> Result<Vec<FileNode>> {
        self.runtime
            .list_files(self.ensure_runtime(box_id).await?, path)
            .await
    }

    async fn read_file(&self, box_id: Uuid, path: &str, limit: usize) -> Result<ReadFileResult> {
        self.runtime
            .read_file(self.ensure_runtime(box_id).await?, path, limit)
            .await
    }

    async fn write_file(
        &self,
        box_id: Uuid,
        path: &str,
        data: &[u8],
        create_parents: bool,
    ) -> Result<()> {
        self.runtime
            .write_file(
                self.ensure_runtime(box_id).await?,
                path,
                data,
                create_parents,
            )
            .await
    }

    async fn make_dir(&self, box_id: Uuid, path: &str, recursive: bool) -> Result<()> {
        self.runtime
            .make_dir(self.ensure_runtime(box_id).await?, path, recursive)
            .await
    }

    async fn remove_path(&self, box_id: Uuid, path: &str, recursive: bool) -> Result<()> {
        self.runtime
            .remove_path(self.ensure_runtime(box_id).await?, path, recursive)
            .await
    }

    async fn exec(&self, box_id: Uuid, request: ExecRequest) -> Result<CommandStream> {
        self.runtime
            .exec(self.ensure_runtime(box_id).await?, request)
            .await
    }

    async fn open_shell(
        &self,
        box_id: Uuid,
        request: ShellRequest,
    ) -> Result<crate::backend::ShellSession> {
        self.runtime
            .open_shell(self.ensure_runtime(box_id).await?, request)
            .await
    }

    async fn checkpoint_create(
        &self,
        box_id: Uuid,
        name: Option<String>,
        metadata: BTreeMap<String, String>,
    ) -> Result<WorkspaceCheckpointRecord> {
        let sandbox_id = self.ensure_runtime(box_id).await?;
        self.runtime
            .capture_workspace_checkpoint(sandbox_id, name, metadata)
            .await
    }

    async fn checkpoint_list(&self, box_id: Uuid) -> Result<Vec<WorkspaceCheckpointRecord>> {
        let _ = self.read_box(box_id).await?;
        self.workspace.list_checkpoints(&box_id.to_string()).await
    }

    async fn checkpoint_restore(
        &self,
        box_id: Uuid,
        checkpoint_id: &str,
        mode: CheckpointRestoreMode,
    ) -> Result<WorkspaceCheckpointRecord> {
        let record = self.read_box(box_id).await?;
        let was_running = record.status == BoxStatus::Running;
        if was_running {
            self.stop_box(box_id).await?;
        }
        let restored = self
            .workspace
            .restore_checkpoint(&box_id.to_string(), checkpoint_id, mode)
            .await?;
        if was_running {
            self.start_box(box_id).await?;
        }
        Ok(restored)
    }

    async fn checkpoint_fork(
        &self,
        box_id: Uuid,
        checkpoint_id: &str,
        new_box_name: Option<String>,
    ) -> Result<BoxRecord> {
        let source = self.read_box(box_id).await?;
        let record = self
            .create_box_record(new_box_name, source.settings.clone())
            .await?;
        self.workspace
            .fork_workspace(
                &box_id.to_string(),
                checkpoint_id,
                &record.box_id.to_string(),
            )
            .await?;
        Ok(record)
    }

    async fn checkpoint_delete(&self, box_id: Uuid, checkpoint_id: &str) -> Result<()> {
        let _ = self.read_box(box_id).await?;
        self.workspace
            .delete_checkpoint(&box_id.to_string(), checkpoint_id)
            .await
    }

    async fn shutdown_daemon(&self) -> Result<()> {
        let box_ids = self.active_box_ids().await;
        for box_id in box_ids {
            self.stop_box_for_shutdown(box_id).await?;
        }
        Ok(())
    }
}
