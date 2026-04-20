use std::collections::BTreeMap;

use uuid::Uuid;

use crate::auth::{AdminCredentialBundle, BoxCredentialBundle};
use crate::boxes::{BoxRecord, BoxSettingValue};
use crate::protocol::OutputStream;
use crate::workspace::CheckpointRestoreMode;
use crate::workspace::{FileNode, ReadFileResult, WorkspaceChange, WorkspaceCheckpointRecord};

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InteractiveTarget {
    Bash,
    Python,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    AuthenticateAdmin {
        admin_uuid: Uuid,
        admin_token: String,
    },
    AuthenticateBox {
        box_id: Uuid,
        box_token: Option<String>,
    },
    Request {
        request: BoxRequest,
    },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Principal {
    Admin { admin_uuid: Uuid },
    Box { box_id: Uuid },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BoxRequest {
    Ping {
        request_id: String,
    },
    ListBoxes {
        request_id: String,
    },
    NewBox {
        request_id: String,
    },
    StartBox {
        request_id: String,
        box_id: Uuid,
    },
    StopBox {
        request_id: String,
        box_id: Uuid,
    },
    RemoveBox {
        request_id: String,
        box_id: Uuid,
    },
    SetBoxSetting {
        request_id: String,
        box_id: Uuid,
        value: BoxSettingValue,
    },
    ExecBash {
        request_id: String,
        box_id: Uuid,
        command: String,
        timeout_ms: Option<u64>,
        kill_grace_ms: Option<u64>,
    },
    ExecPython {
        request_id: String,
        box_id: Uuid,
        args: Vec<String>,
        timeout_ms: Option<u64>,
        kill_grace_ms: Option<u64>,
    },
    OpenShell {
        request_id: String,
        box_id: Uuid,
        target: InteractiveTarget,
    },
    ShellInput {
        request_id: String,
        shell_id: Uuid,
        data: String,
    },
    ResizeShell {
        request_id: String,
        shell_id: Uuid,
        cols: u16,
        rows: u16,
    },
    CloseShell {
        request_id: String,
        shell_id: Uuid,
    },
    FsList {
        request_id: String,
        box_id: Uuid,
        path: String,
    },
    FsRead {
        request_id: String,
        box_id: Uuid,
        path: String,
        limit: usize,
    },
    FsWrite {
        request_id: String,
        box_id: Uuid,
        path: String,
        data: String,
        create_parents: bool,
    },
    FsMkdir {
        request_id: String,
        box_id: Uuid,
        path: String,
        recursive: bool,
    },
    FsRemove {
        request_id: String,
        box_id: Uuid,
        path: String,
        recursive: bool,
    },
    FsDiff {
        request_id: String,
        box_id: Uuid,
    },
    CheckpointCreate {
        request_id: String,
        box_id: Uuid,
        name: Option<String>,
        metadata: BTreeMap<String, String>,
    },
    CheckpointList {
        request_id: String,
        box_id: Uuid,
    },
    CheckpointRestore {
        request_id: String,
        box_id: Uuid,
        checkpoint_id: String,
        mode: CheckpointRestoreMode,
    },
    CheckpointFork {
        request_id: String,
        box_id: Uuid,
        checkpoint_id: String,
        new_box_name: Option<String>,
    },
    CheckpointDelete {
        request_id: String,
        box_id: Uuid,
        checkpoint_id: String,
    },
    ShutdownDaemon {
        request_id: String,
    },
    AdminAdd {
        request_id: String,
    },
    BoxIssueCredentials {
        request_id: String,
        box_id: Uuid,
    },
    AdminRemoveMe {
        request_id: String,
    },
}

impl BoxRequest {
    pub fn request_id(&self) -> &str {
        match self {
            Self::Ping { request_id }
            | Self::ListBoxes { request_id }
            | Self::NewBox { request_id }
            | Self::StartBox { request_id, .. }
            | Self::StopBox { request_id, .. }
            | Self::RemoveBox { request_id, .. }
            | Self::SetBoxSetting { request_id, .. }
            | Self::ExecBash { request_id, .. }
            | Self::ExecPython { request_id, .. }
            | Self::OpenShell { request_id, .. }
            | Self::ShellInput { request_id, .. }
            | Self::ResizeShell { request_id, .. }
            | Self::CloseShell { request_id, .. }
            | Self::FsList { request_id, .. }
            | Self::FsRead { request_id, .. }
            | Self::FsWrite { request_id, .. }
            | Self::FsMkdir { request_id, .. }
            | Self::FsRemove { request_id, .. }
            | Self::FsDiff { request_id, .. }
            | Self::CheckpointCreate { request_id, .. }
            | Self::CheckpointList { request_id, .. }
            | Self::CheckpointRestore { request_id, .. }
            | Self::CheckpointFork { request_id, .. }
            | Self::CheckpointDelete { request_id, .. }
            | Self::ShutdownDaemon { request_id }
            | Self::AdminAdd { request_id }
            | Self::BoxIssueCredentials { request_id, .. }
            | Self::AdminRemoveMe { request_id } => request_id,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BoxResponse {
    Pong,
    BoxList {
        boxes: Vec<BoxRecord>,
    },
    Box {
        record: BoxRecord,
    },
    BoxRemoved {
        box_id: Uuid,
    },
    Files {
        path: String,
        entries: Vec<FileNode>,
    },
    File {
        file: ReadFileResult,
    },
    Changes {
        changes: Vec<WorkspaceChange>,
    },
    Checkpoint {
        checkpoint: WorkspaceCheckpointRecord,
    },
    CheckpointList {
        checkpoints: Vec<WorkspaceCheckpointRecord>,
    },
    ShellOpened {
        shell_id: Uuid,
        box_id: Uuid,
    },
    AdminAdded {
        bundle: AdminCredentialBundle,
    },
    BoxCredentials {
        bundle: BoxCredentialBundle,
    },
    Ack,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    Authenticated { principal: Principal },
    Event { event: Box<BoxEvent> },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BoxEvent {
    Response {
        request_id: String,
        response: Box<BoxResponse>,
    },
    ExecOutput {
        request_id: String,
        stream: OutputStream,
        data: String,
    },
    ExecExit {
        request_id: String,
        status: crate::protocol::ExecExit,
    },
    ShellOutput {
        shell_id: Uuid,
        data: String,
    },
    ShellExit {
        shell_id: Uuid,
        code: i32,
    },
    Error {
        request_id: Option<String>,
        message: String,
    },
}
