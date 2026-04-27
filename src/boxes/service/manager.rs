use std::collections::BTreeMap;

use async_trait::async_trait;
use uuid::Uuid;

use crate::Result;
use crate::protocol::{CommandStream, ExecRequest, ShellRequest};
use crate::workspace::{
    CheckpointRestoreMode, FileNode, ReadFileResult, WorkspaceCheckpointRecord,
};

use super::super::{BoxRecord, BoxSettingValue};

#[async_trait]
pub trait BoxManager: Send + Sync {
    async fn list_boxes(&self) -> Result<Vec<BoxRecord>>;
    async fn get_box(&self, box_id: Uuid) -> Result<BoxRecord>;
    async fn create_box(&self) -> Result<BoxRecord>;
    async fn create_named_box(&self, name: Option<String>) -> Result<BoxRecord>;
    async fn set_box_setting(&self, box_id: Uuid, setting: BoxSettingValue) -> Result<BoxRecord>;
    async fn start_box(&self, box_id: Uuid) -> Result<BoxRecord>;
    async fn stop_box(&self, box_id: Uuid) -> Result<BoxRecord>;
    async fn remove_box(&self, box_id: Uuid) -> Result<()>;
    async fn list_files(&self, box_id: Uuid, path: &str) -> Result<Vec<FileNode>>;
    async fn read_file(&self, box_id: Uuid, path: &str, limit: usize) -> Result<ReadFileResult>;
    async fn write_file(
        &self,
        box_id: Uuid,
        path: &str,
        data: &[u8],
        create_parents: bool,
    ) -> Result<()>;
    async fn make_dir(&self, box_id: Uuid, path: &str, recursive: bool) -> Result<()>;
    async fn remove_path(&self, box_id: Uuid, path: &str, recursive: bool) -> Result<()>;
    async fn exec(&self, box_id: Uuid, request: ExecRequest) -> Result<CommandStream>;
    async fn open_shell(
        &self,
        box_id: Uuid,
        request: ShellRequest,
    ) -> Result<crate::backend::ShellSession>;
    async fn checkpoint_create(
        &self,
        box_id: Uuid,
        name: Option<String>,
        metadata: BTreeMap<String, String>,
    ) -> Result<WorkspaceCheckpointRecord>;
    async fn checkpoint_list(&self, box_id: Uuid) -> Result<Vec<WorkspaceCheckpointRecord>>;
    async fn checkpoint_restore(
        &self,
        box_id: Uuid,
        checkpoint_id: &str,
        mode: CheckpointRestoreMode,
    ) -> Result<WorkspaceCheckpointRecord>;
    async fn checkpoint_fork(
        &self,
        box_id: Uuid,
        checkpoint_id: &str,
        new_box_name: Option<String>,
    ) -> Result<BoxRecord>;
    async fn checkpoint_delete(&self, box_id: Uuid, checkpoint_id: &str) -> Result<()>;
    async fn shutdown_daemon(&self) -> Result<()>;
}
