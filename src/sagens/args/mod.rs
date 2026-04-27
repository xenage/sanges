mod help;
mod help_pages;
mod parse;
mod parse_box;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use uuid::Uuid;

use crate::box_api::InteractiveTarget;
use crate::{BoxSettingValue, CheckpointRestoreMode};

pub use help::render_help;
pub use parse::parse;

pub enum Command {
    Help(HelpTopic),
    Start,
    Quit,
    Update,
    Daemon(DaemonCommand),
    Admin(AdminCommand),
    Box(BoxCommand),
}

#[derive(Debug, Clone, Copy)]
pub enum HelpTopic {
    Root,
    Start,
    Quit,
    Update,
    Daemon,
    DaemonLog,
    Admin,
    AdminAdd,
    AdminRemoveMe,
    Box,
    BoxList,
    BoxNew,
    BoxStart,
    BoxStop,
    BoxRemove,
    BoxSet,
    BoxExec,
    BoxFs,
    BoxFsList,
    BoxFsUpload,
    BoxFsDownload,
    BoxCheckpoint,
    BoxCheckpointCreate,
    BoxCheckpointList,
    BoxCheckpointRestore,
    BoxCheckpointFork,
    BoxCheckpointDelete,
}

pub enum AdminCommand {
    Add,
    RemoveMe,
}

pub enum DaemonCommand {
    Run,
    Log(DaemonLogCommand),
}

pub struct DaemonLogCommand {
    pub tail: Option<usize>,
    pub follow: bool,
}

pub enum BoxCommand {
    List,
    New,
    Start(Uuid),
    Stop(Uuid),
    Remove(Uuid),
    Set(BoxSetCommand),
    Exec(ExecCommand),
    Fs(FsCommand),
    Checkpoint(CheckpointCommand),
}

pub struct BoxSetCommand {
    pub box_id: Option<Uuid>,
    pub value: BoxSettingValue,
}

pub struct ExecCommand {
    pub box_id: Uuid,
    pub target: ExecTarget,
}

pub enum ExecTarget {
    Bash(String),
    Python(Vec<String>),
    Interactive(InteractiveTarget),
}

pub enum FsCommand {
    List {
        box_id: Uuid,
        path: String,
    },
    Upload {
        box_id: Uuid,
        local_path: String,
        remote_path: String,
    },
    Download {
        box_id: Uuid,
        remote_path: String,
        local_path: String,
    },
}

pub enum CheckpointCommand {
    Create {
        box_id: Uuid,
        name: Option<String>,
        metadata: BTreeMap<String, String>,
    },
    List {
        box_id: Uuid,
    },
    Restore {
        box_id: Uuid,
        checkpoint_id: String,
        mode: CheckpointRestoreMode,
    },
    Fork {
        box_id: Uuid,
        checkpoint_id: String,
        new_box_name: Option<String>,
    },
    Delete {
        box_id: Uuid,
        checkpoint_id: String,
    },
}
