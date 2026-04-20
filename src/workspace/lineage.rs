use std::path::{Path, PathBuf};

use async_trait::async_trait;
use tokio::fs;

use crate::workspace::validate_persisted_id;
use crate::workspace::{WorkspaceCheckpointRecord, WorkspaceLease};
use crate::{Result, SandboxError};

#[async_trait]
pub trait WorkspaceLineageStore: Send + Sync {
    async fn ensure_workspace(&self, workspace_id: &str) -> Result<()>;
    async fn head_checkpoint_id(&self, workspace_id: &str) -> Result<Option<String>>;
    async fn set_head_checkpoint_id(
        &self,
        workspace_id: &str,
        checkpoint_id: Option<&str>,
    ) -> Result<()>;
    async fn save_checkpoint(
        &self,
        workspace: &WorkspaceLease,
        checkpoint: &WorkspaceCheckpointRecord,
    ) -> Result<()>;
    async fn load_checkpoint(
        &self,
        workspace_id: &str,
        checkpoint_id: &str,
    ) -> Result<Option<WorkspaceCheckpointRecord>>;
    async fn list_checkpoints(&self, workspace_id: &str) -> Result<Vec<WorkspaceCheckpointRecord>>;
    async fn delete_checkpoint(&self, workspace_id: &str, checkpoint_id: &str) -> Result<()>;
    async fn copy_checkpoint_snapshot(
        &self,
        workspace_id: &str,
        checkpoint_id: &str,
        destination: &Path,
    ) -> Result<()>;
}

#[derive(Debug, Clone)]
pub struct LocalLineageStore {
    checkpoints_dir: PathBuf,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
struct WorkspaceLineageHead {
    head_checkpoint_id: Option<String>,
}

impl LocalLineageStore {
    pub fn new(checkpoints_dir: impl Into<PathBuf>) -> Self {
        Self {
            checkpoints_dir: checkpoints_dir.into(),
        }
    }

    fn workspace_dir(&self, workspace_id: &str) -> PathBuf {
        self.checkpoints_dir.join(workspace_id)
    }

    fn checkpoint_dir(&self, workspace_id: &str, checkpoint_id: &str) -> PathBuf {
        self.workspace_dir(workspace_id).join(checkpoint_id)
    }

    fn checkpoint_record_path(&self, workspace_id: &str, checkpoint_id: &str) -> PathBuf {
        self.checkpoint_dir(workspace_id, checkpoint_id)
            .join("record.json")
    }

    fn checkpoint_snapshot_path(&self, workspace_id: &str, checkpoint_id: &str) -> PathBuf {
        self.checkpoint_dir(workspace_id, checkpoint_id)
            .join("workspace.raw")
    }

    fn head_path(&self, workspace_id: &str) -> PathBuf {
        self.workspace_dir(workspace_id).join("head.json")
    }

    async fn list_checkpoint_records(
        &self,
        workspace_id: &str,
    ) -> Result<Vec<WorkspaceCheckpointRecord>> {
        let dir = self.workspace_dir(workspace_id);
        if !fs::try_exists(&dir)
            .await
            .map_err(|error| SandboxError::io("checking workspace checkpoints", error))?
        {
            return Ok(Vec::new());
        }
        let mut entries = fs::read_dir(&dir)
            .await
            .map_err(|error| SandboxError::io("reading workspace checkpoints", error))?;
        let mut records = Vec::new();
        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|error| SandboxError::io("iterating workspace checkpoints", error))?
        {
            if !entry
                .file_type()
                .await
                .map_err(|error| {
                    SandboxError::io("reading workspace checkpoint entry type", error)
                })?
                .is_dir()
            {
                continue;
            }
            let record_path = entry.path().join("record.json");
            if !fs::try_exists(&record_path)
                .await
                .map_err(|error| SandboxError::io("checking checkpoint record", error))?
            {
                continue;
            }
            records.push(self.read_json(&record_path).await?);
        }
        records.sort_by_key(|record: &WorkspaceCheckpointRecord| {
            (
                record.summary.created_at_ms,
                record.summary.checkpoint_id.clone(),
            )
        });
        Ok(records)
    }

    async fn write_json<T: serde::Serialize>(&self, path: &Path, value: &T) -> Result<()> {
        let bytes = serde_json::to_vec_pretty(value)
            .map_err(|error| SandboxError::json("encoding workspace metadata", error))?;
        fs::write(path, bytes)
            .await
            .map_err(|error| SandboxError::io("writing workspace metadata", error))
    }

    async fn read_json<T: serde::de::DeserializeOwned>(&self, path: &Path) -> Result<T> {
        let bytes = fs::read(path)
            .await
            .map_err(|error| SandboxError::io("reading workspace metadata", error))?;
        serde_json::from_slice(&bytes)
            .map_err(|error| SandboxError::json("decoding workspace metadata", error))
    }

    async fn copy_file(&self, source: &Path, destination: &Path, context: &str) -> Result<()> {
        fs::copy(source, destination)
            .await
            .map_err(|error| SandboxError::io(format!("copying disk for {context}"), error))?;
        Ok(())
    }
}

#[async_trait]
impl WorkspaceLineageStore for LocalLineageStore {
    async fn ensure_workspace(&self, workspace_id: &str) -> Result<()> {
        validate_persisted_id(workspace_id, "workspace_id")?;
        let dir = self.workspace_dir(workspace_id);
        fs::create_dir_all(&dir)
            .await
            .map_err(|error| SandboxError::io("creating workspace checkpoint directory", error))?;
        let head_path = self.head_path(workspace_id);
        if !fs::try_exists(&head_path)
            .await
            .map_err(|error| SandboxError::io("checking workspace checkpoint head", error))?
        {
            let head = WorkspaceLineageHead {
                head_checkpoint_id: self
                    .list_checkpoint_records(workspace_id)
                    .await?
                    .last()
                    .map(|record| record.summary.checkpoint_id.clone()),
            };
            self.write_json(&head_path, &head).await?;
        }
        Ok(())
    }

    async fn head_checkpoint_id(&self, workspace_id: &str) -> Result<Option<String>> {
        validate_persisted_id(workspace_id, "workspace_id")?;
        let head_path = self.head_path(workspace_id);
        if fs::try_exists(&head_path)
            .await
            .map_err(|error| SandboxError::io("checking workspace checkpoint head", error))?
        {
            let head: WorkspaceLineageHead = self.read_json(&head_path).await?;
            return Ok(head.head_checkpoint_id);
        }
        Ok(self
            .list_checkpoint_records(workspace_id)
            .await?
            .last()
            .map(|record| record.summary.checkpoint_id.clone()))
    }

    async fn set_head_checkpoint_id(
        &self,
        workspace_id: &str,
        checkpoint_id: Option<&str>,
    ) -> Result<()> {
        validate_persisted_id(workspace_id, "workspace_id")?;
        if let Some(checkpoint_id) = checkpoint_id {
            validate_persisted_id(checkpoint_id, "checkpoint_id")?;
        }
        self.ensure_workspace(workspace_id).await?;
        self.write_json(
            &self.head_path(workspace_id),
            &WorkspaceLineageHead {
                head_checkpoint_id: checkpoint_id.map(str::to_owned),
            },
        )
        .await
    }

    async fn save_checkpoint(
        &self,
        workspace: &WorkspaceLease,
        checkpoint: &WorkspaceCheckpointRecord,
    ) -> Result<()> {
        validate_persisted_id(&workspace.workspace_id, "workspace_id")?;
        validate_persisted_id(&checkpoint.summary.workspace_id, "workspace_id")?;
        validate_persisted_id(&checkpoint.summary.checkpoint_id, "checkpoint_id")?;
        if let Some(source_checkpoint_id) = checkpoint.source_checkpoint_id.as_deref() {
            validate_persisted_id(source_checkpoint_id, "source_checkpoint_id")?;
        }
        if checkpoint.summary.workspace_id != workspace.workspace_id {
            return Err(SandboxError::invalid(format!(
                "checkpoint workspace {} does not match lease {}",
                checkpoint.summary.workspace_id, workspace.workspace_id
            )));
        }
        self.ensure_workspace(&workspace.workspace_id).await?;
        let checkpoint_dir =
            self.checkpoint_dir(&workspace.workspace_id, &checkpoint.summary.checkpoint_id);
        fs::create_dir_all(&checkpoint_dir)
            .await
            .map_err(|error| SandboxError::io("creating workspace checkpoint directory", error))?;
        self.copy_file(
            &workspace.disk_path,
            &self.checkpoint_snapshot_path(
                &workspace.workspace_id,
                &checkpoint.summary.checkpoint_id,
            ),
            "checkpoint",
        )
        .await?;
        self.write_json(
            &self
                .checkpoint_record_path(&workspace.workspace_id, &checkpoint.summary.checkpoint_id),
            checkpoint,
        )
        .await
    }

    async fn load_checkpoint(
        &self,
        workspace_id: &str,
        checkpoint_id: &str,
    ) -> Result<Option<WorkspaceCheckpointRecord>> {
        validate_persisted_id(workspace_id, "workspace_id")?;
        validate_persisted_id(checkpoint_id, "checkpoint_id")?;
        let record_path = self.checkpoint_record_path(workspace_id, checkpoint_id);
        if !fs::try_exists(&record_path)
            .await
            .map_err(|error| SandboxError::io("checking checkpoint record", error))?
        {
            return Ok(None);
        }
        self.read_json(&record_path).await.map(Some)
    }

    async fn list_checkpoints(&self, workspace_id: &str) -> Result<Vec<WorkspaceCheckpointRecord>> {
        validate_persisted_id(workspace_id, "workspace_id")?;
        self.list_checkpoint_records(workspace_id).await
    }

    async fn delete_checkpoint(&self, workspace_id: &str, checkpoint_id: &str) -> Result<()> {
        validate_persisted_id(workspace_id, "workspace_id")?;
        validate_persisted_id(checkpoint_id, "checkpoint_id")?;
        let checkpoint_dir = self.checkpoint_dir(workspace_id, checkpoint_id);
        if fs::try_exists(&checkpoint_dir)
            .await
            .map_err(|error| SandboxError::io("checking checkpoint directory", error))?
        {
            fs::remove_dir_all(&checkpoint_dir)
                .await
                .map_err(|error| SandboxError::io("removing checkpoint directory", error))?;
        }
        Ok(())
    }

    async fn copy_checkpoint_snapshot(
        &self,
        workspace_id: &str,
        checkpoint_id: &str,
        destination: &Path,
    ) -> Result<()> {
        validate_persisted_id(workspace_id, "workspace_id")?;
        validate_persisted_id(checkpoint_id, "checkpoint_id")?;
        self.copy_file(
            &self.checkpoint_snapshot_path(workspace_id, checkpoint_id),
            destination,
            "checkpoint workspace",
        )
        .await
    }
}
