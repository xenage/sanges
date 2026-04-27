use std::collections::BTreeMap;

use uuid::Uuid;

use crate::workspace::validate_persisted_id;
use crate::workspace::{WorkspaceChange, WorkspaceSnapshot};
use crate::{Result, SandboxError};

use super::WorkspaceLease;
use super::WorkspaceLineageStore;
use super::store::WorkspaceStore;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WorkspaceCheckpointSummary {
    pub checkpoint_id: String,
    pub workspace_id: String,
    pub name: Option<String>,
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WorkspaceCheckpointRecord {
    pub summary: WorkspaceCheckpointSummary,
    pub source_checkpoint_id: Option<String>,
    pub changes: Vec<WorkspaceChange>,
}

pub type WorkspaceCommitSummary = WorkspaceCheckpointSummary;
pub type WorkspaceCommitRecord = WorkspaceCheckpointRecord;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckpointRestoreMode {
    Rollback,
    Replace,
}

impl WorkspaceStore {
    pub async fn create_checkpoint(
        &self,
        workspace: &WorkspaceLease,
        changes: Vec<WorkspaceChange>,
        name: Option<String>,
        metadata: BTreeMap<String, String>,
    ) -> Result<WorkspaceCheckpointRecord> {
        let checkpoint_id = Uuid::new_v4().to_string();
        let source_checkpoint_id = self
            .lineage
            .head_checkpoint_id(&workspace.workspace_id)
            .await?;
        let record = WorkspaceCheckpointRecord {
            summary: WorkspaceCheckpointSummary {
                checkpoint_id: checkpoint_id.clone(),
                workspace_id: workspace.workspace_id.clone(),
                name,
                metadata,
                created_at_ms: super::store::now_ms(),
            },
            source_checkpoint_id,
            changes,
        };
        self.lineage.save_checkpoint(workspace, &record).await?;
        self.lineage
            .set_head_checkpoint_id(&workspace.workspace_id, Some(&checkpoint_id))
            .await?;
        Ok(record)
    }

    pub async fn list_checkpoints(
        &self,
        workspace_id: &str,
    ) -> Result<Vec<WorkspaceCheckpointRecord>> {
        validate_persisted_id(workspace_id, "workspace_id")?;
        self.lineage.list_checkpoints(workspace_id).await
    }

    pub async fn restore_checkpoint(
        &self,
        workspace_id: &str,
        checkpoint_id: &str,
        mode: CheckpointRestoreMode,
    ) -> Result<WorkspaceCheckpointRecord> {
        validate_persisted_id(workspace_id, "workspace_id")?;
        validate_persisted_id(checkpoint_id, "checkpoint_id")?;
        let lease = self.prepare_workspace(workspace_id).await?;
        let checkpoints = self.lineage.list_checkpoints(workspace_id).await?;
        let target_index = checkpoints
            .iter()
            .position(|record| record.summary.checkpoint_id == checkpoint_id)
            .ok_or_else(|| {
                SandboxError::not_found(format!("unknown checkpoint {checkpoint_id}"))
            })?;
        let target = checkpoints[target_index].clone();
        self.lineage
            .copy_checkpoint_snapshot(workspace_id, checkpoint_id, &lease.disk_path)
            .await?;
        if mode == CheckpointRestoreMode::Rollback {
            let newer_checkpoint_ids = checkpoints[target_index + 1..]
                .iter()
                .map(|record| record.summary.checkpoint_id.as_str())
                .collect::<Vec<_>>();
            self.delete_checkpoint_ids(workspace_id, newer_checkpoint_ids)
                .await?;
        }
        self.lineage
            .set_head_checkpoint_id(workspace_id, Some(checkpoint_id))
            .await?;
        Ok(target)
    }

    pub async fn delete_checkpoint(&self, workspace_id: &str, checkpoint_id: &str) -> Result<()> {
        validate_persisted_id(workspace_id, "workspace_id")?;
        validate_persisted_id(checkpoint_id, "checkpoint_id")?;

        let current_head = self.lineage.head_checkpoint_id(workspace_id).await?;
        let record = self
            .lineage
            .load_checkpoint(workspace_id, checkpoint_id)
            .await?;
        self.lineage
            .delete_checkpoint(workspace_id, checkpoint_id)
            .await?;
        if current_head.as_deref() == Some(checkpoint_id) {
            let next_head = record.and_then(|record| record.source_checkpoint_id);
            self.lineage
                .set_head_checkpoint_id(workspace_id, next_head.as_deref())
                .await?;
        }
        Ok(())
    }

    pub async fn fork_workspace(
        &self,
        source_workspace_id: &str,
        checkpoint_id: &str,
        new_workspace_id: &str,
    ) -> Result<WorkspaceCheckpointRecord> {
        validate_persisted_id(source_workspace_id, "workspace_id")?;
        validate_persisted_id(checkpoint_id, "checkpoint_id")?;
        validate_persisted_id(new_workspace_id, "new_workspace_id")?;

        let record = self
            .load_checkpoint_record(source_workspace_id, checkpoint_id)
            .await?;
        let lease = self.prepare_workspace(new_workspace_id).await?;
        self.lineage
            .copy_checkpoint_snapshot(source_workspace_id, checkpoint_id, &lease.disk_path)
            .await?;
        self.lineage
            .set_head_checkpoint_id(new_workspace_id, None)
            .await?;
        Ok(record)
    }

    pub(crate) async fn create_internal_checkpoint(
        &self,
        workspace: &WorkspaceLease,
        baseline: &WorkspaceSnapshot,
        current: &WorkspaceSnapshot,
        name: Option<String>,
        metadata: BTreeMap<String, String>,
    ) -> Result<WorkspaceCommitRecord> {
        self.create_checkpoint(workspace, baseline.diff(current), name, metadata)
            .await
    }

    pub(crate) async fn restore_internal_checkpoint(
        &self,
        workspace_id: &str,
        checkpoint_id: &str,
    ) -> Result<WorkspaceCommitRecord> {
        self.restore_checkpoint(workspace_id, checkpoint_id, CheckpointRestoreMode::Rollback)
            .await
    }

    async fn load_checkpoint_record(
        &self,
        workspace_id: &str,
        checkpoint_id: &str,
    ) -> Result<WorkspaceCheckpointRecord> {
        self.lineage
            .load_checkpoint(workspace_id, checkpoint_id)
            .await?
            .ok_or_else(|| SandboxError::not_found(format!("unknown checkpoint {checkpoint_id}")))
    }

    async fn delete_checkpoint_ids<'a, I>(
        &self,
        workspace_id: &str,
        checkpoint_ids: I,
    ) -> Result<()>
    where
        I: IntoIterator<Item = &'a str>,
    {
        for checkpoint_id in checkpoint_ids {
            self.lineage
                .delete_checkpoint(workspace_id, checkpoint_id)
                .await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
