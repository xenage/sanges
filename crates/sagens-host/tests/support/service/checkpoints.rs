use std::collections::BTreeMap;

use sagens_host::workspace::WorkspaceCheckpointRecord;
use sagens_host::workspace::{FileKind, FileNode, WorkspaceChange, WorkspaceChangeKind};
use uuid::Uuid;

use super::state::StubState;

pub(super) fn remove_box_snapshots(state: &mut StubState, box_id: Uuid) {
    state
        .checkpoint_snapshots
        .retain(|(snapshot_box_id, _), _| *snapshot_box_id != box_id);
}

pub(super) fn remove_box_state(state: &mut StubState, box_id: Uuid) {
    state.boxes.remove(&box_id);
    state.changes.remove(&box_id);
    state.files.remove(&box_id);
    state.checkpoints.remove(&box_id);
    remove_box_snapshots(state, box_id);
    state.checkpoint_heads.remove(&box_id);
    state.file_data.retain(|(id, _), _| *id != box_id);
}

pub(super) fn capture_snapshot(state: &StubState, box_id: Uuid) -> BTreeMap<String, Vec<u8>> {
    let mut snapshot = BTreeMap::new();
    for ((snapshot_box_id, path), data) in &state.file_data {
        if *snapshot_box_id == box_id {
            snapshot.insert(path.clone(), data.clone());
        }
    }
    snapshot
}

pub(super) fn checkpoint_exists(state: &StubState, box_id: Uuid, checkpoint_id: &str) -> bool {
    let Some(items) = state.checkpoints.get(&box_id) else {
        return false;
    };
    items
        .iter()
        .any(|item| item.summary.checkpoint_id == checkpoint_id)
}

pub(super) fn rollback_checkpoints(state: &mut StubState, box_id: Uuid, checkpoint_id: &str) {
    let Some(checkpoints): Option<&mut Vec<WorkspaceCheckpointRecord>> =
        state.checkpoints.get_mut(&box_id)
    else {
        unreachable!("checkpoint exists if restore found it");
    };
    let target_index = checkpoints
        .iter()
        .position(|item| item.summary.checkpoint_id == checkpoint_id)
        .expect("checkpoint index");
    let removed = checkpoints
        .drain(target_index + 1..)
        .map(|item| item.summary.checkpoint_id)
        .collect::<Vec<_>>();
    for removed_checkpoint_id in removed {
        state
            .checkpoint_snapshots
            .remove(&(box_id, removed_checkpoint_id));
    }
}

pub(super) fn apply_snapshot(
    state: &mut StubState,
    box_id: Uuid,
    snapshot: &BTreeMap<String, Vec<u8>>,
) {
    state.file_data.retain(|(id, _), _| *id != box_id);
    for (path, data) in snapshot {
        state.file_data.insert((box_id, path.clone()), data.clone());
    }
    state.files.insert(box_id, file_nodes(snapshot));
    state.changes.insert(box_id, workspace_changes(snapshot));
}

fn file_nodes(snapshot: &BTreeMap<String, Vec<u8>>) -> Vec<FileNode> {
    snapshot
        .iter()
        .map(|(path, data)| FileNode {
            path: path.trim_start_matches("/workspace/").into(),
            kind: FileKind::File,
            size: data.len() as u64,
            digest: Some("digest".into()),
            target: None,
        })
        .collect()
}

fn workspace_changes(snapshot: &BTreeMap<String, Vec<u8>>) -> Vec<WorkspaceChange> {
    snapshot
        .keys()
        .map(|path| WorkspaceChange {
            path: path.trim_start_matches("/workspace/").into(),
            kind: WorkspaceChangeKind::Added,
            kind_after: Some(FileKind::File),
        })
        .collect()
}
