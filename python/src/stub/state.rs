use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use sagens_host::boxes::{BoxBooleanSetting, BoxNumericSetting, BoxRecord, BoxSettings};
use sagens_host::workspace::{FileKind, FileNode, WorkspaceCheckpointRecord};
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(Default)]
pub(super) struct StubState {
    pub(super) boxes: HashMap<Uuid, BoxRecord>,
    pub(super) files: HashMap<Uuid, Vec<FileNode>>,
    pub(super) file_data: HashMap<(Uuid, String), Vec<u8>>,
    pub(super) committed: u64,
    pub(super) checkpoints: HashMap<Uuid, Vec<WorkspaceCheckpointRecord>>,
    pub(super) checkpoint_snapshots: HashMap<(Uuid, String), BTreeMap<String, Vec<u8>>>,
    pub(super) checkpoint_heads: HashMap<Uuid, String>,
}

pub(crate) struct StubBoxManager {
    pub(super) state: Mutex<StubState>,
    pub(super) exec_running: Arc<Mutex<HashSet<Uuid>>>,
}

impl Default for StubBoxManager {
    fn default() -> Self {
        Self {
            state: Mutex::new(StubState::default()),
            exec_running: Arc::new(Mutex::new(HashSet::new())),
        }
    }
}

pub(super) fn default_box_settings() -> BoxSettings {
    BoxSettings {
        cpu_cores: BoxNumericSetting { current: 1, max: 8 },
        memory_mb: BoxNumericSetting {
            current: 256,
            max: 8192,
        },
        fs_size_mib: BoxNumericSetting {
            current: 128,
            max: 8192,
        },
        max_processes: BoxNumericSetting {
            current: 256,
            max: 4096,
        },
        network_enabled: BoxBooleanSetting {
            current: false,
            max: true,
        },
    }
}

pub(super) fn has_workspace_file(state: &StubState, box_id: Uuid, path: &str) -> bool {
    state
        .file_data
        .contains_key(&(box_id, normalize_workspace_path(path)))
}

pub(super) fn read_workspace_file(state: &StubState, box_id: Uuid, path: &str) -> Vec<u8> {
    state
        .file_data
        .get(&(box_id, normalize_workspace_path(path)))
        .cloned()
        .unwrap_or_default()
}

pub(super) fn set_workspace_file(state: &mut StubState, box_id: Uuid, path: &str, data: Vec<u8>) {
    state
        .file_data
        .insert((box_id, normalize_workspace_path(path)), data);
    refresh_workspace(state, box_id);
}

fn refresh_workspace(state: &mut StubState, box_id: Uuid) {
    let mut paths = state
        .file_data
        .iter()
        .filter(|((id, _), _)| *id == box_id)
        .map(|((_, path), data)| {
            (
                path.trim_start_matches("/workspace/").to_string(),
                data.len(),
            )
        })
        .collect::<Vec<_>>();
    paths.sort_by(|left, right| left.0.cmp(&right.0));
    state.files.insert(
        box_id,
        paths
            .iter()
            .map(|(path, len)| FileNode {
                path: path.clone(),
                kind: FileKind::File,
                size: *len as u64,
                digest: Some("digest".into()),
                target: None,
            })
            .collect(),
    );
}

fn normalize_workspace_path(path: &str) -> String {
    if path.starts_with("/workspace/") {
        return path.into();
    }
    if path == "/workspace" {
        return "/workspace".into();
    }
    format!("/workspace/{}", path.trim_start_matches("./"))
}
