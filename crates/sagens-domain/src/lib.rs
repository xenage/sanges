use async_trait::async_trait;
use uuid::Uuid;

pub type BoxId = Uuid;
pub type SandboxId = Uuid;

#[async_trait]
pub trait BoxCatalog {
    type Error;
    type BoxRecord;

    async fn list_boxes(&self) -> Result<Vec<Self::BoxRecord>, Self::Error>;
    async fn create_box(&self, name: Option<String>) -> Result<Self::BoxRecord, Self::Error>;
}

#[async_trait]
pub trait BoxLifecycle {
    type Error;
    type BoxRecord;

    async fn start_box(&self, box_id: BoxId) -> Result<Self::BoxRecord, Self::Error>;
    async fn stop_box(&self, box_id: BoxId) -> Result<Self::BoxRecord, Self::Error>;
    async fn remove_box(&self, box_id: BoxId) -> Result<(), Self::Error>;
}

#[async_trait]
pub trait BoxFilesystem {
    type Error;
    type Change;
    type DirectoryEntry;
    type FileRead;

    async fn list_files(
        &self,
        box_id: BoxId,
        path: &str,
    ) -> Result<Vec<Self::DirectoryEntry>, Self::Error>;
    async fn read_file(
        &self,
        box_id: BoxId,
        path: &str,
        limit: usize,
    ) -> Result<Self::FileRead, Self::Error>;
    async fn list_changes(&self, box_id: BoxId) -> Result<Vec<Self::Change>, Self::Error>;
    async fn write_file(
        &self,
        box_id: BoxId,
        path: &str,
        data: &[u8],
        create_parents: bool,
    ) -> Result<(), Self::Error>;
    async fn make_dir(&self, box_id: BoxId, path: &str, recursive: bool)
    -> Result<(), Self::Error>;
    async fn remove_path(
        &self,
        box_id: BoxId,
        path: &str,
        recursive: bool,
    ) -> Result<(), Self::Error>;
}

#[async_trait]
pub trait BoxExecution {
    type Error;
    type CommandStream;
    type ShellRequest;
    type ShellSession;
    type ExecRequest;

    async fn exec(
        &self,
        box_id: BoxId,
        request: Self::ExecRequest,
    ) -> Result<Self::CommandStream, Self::Error>;
    async fn open_shell(
        &self,
        box_id: BoxId,
        request: Self::ShellRequest,
    ) -> Result<Self::ShellSession, Self::Error>;
}

#[async_trait]
pub trait CheckpointLineage {
    type Error;
    type BoxRecord;
    type CheckpointRecord;
    type RestoreMode;

    async fn checkpoint_create(
        &self,
        box_id: BoxId,
        name: Option<String>,
        metadata: std::collections::BTreeMap<String, String>,
    ) -> Result<Self::CheckpointRecord, Self::Error>;
    async fn checkpoint_list(
        &self,
        box_id: BoxId,
    ) -> Result<Vec<Self::CheckpointRecord>, Self::Error>;
    async fn checkpoint_restore(
        &self,
        box_id: BoxId,
        checkpoint_id: &str,
        mode: Self::RestoreMode,
    ) -> Result<Self::CheckpointRecord, Self::Error>;
    async fn checkpoint_fork(
        &self,
        box_id: BoxId,
        checkpoint_id: &str,
        new_box_name: Option<String>,
    ) -> Result<Self::BoxRecord, Self::Error>;
    async fn checkpoint_delete(
        &self,
        box_id: BoxId,
        checkpoint_id: &str,
    ) -> Result<(), Self::Error>;
}

#[async_trait]
pub trait AdminControl {
    type Error;

    async fn shutdown_daemon(&self) -> Result<(), Self::Error>;
}
