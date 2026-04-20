use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::time::Duration;

use tempfile::tempdir;
use tokio::time::sleep;

use crate::config::WorkspaceConfig;

use super::{CheckpointRestoreMode, WorkspaceStore};

const WORKSPACE_ID: &str = "workspace-a";
const SOURCE_WORKSPACE_ID: &str = "workspace-source";
const FORK_WORKSPACE_ID: &str = "workspace-fork";

#[tokio::test]
async fn rollback_prunes_newer_checkpoints_but_keeps_target() {
    let temp = tempdir().expect("tempdir");
    let store = WorkspaceStore::new(temp.path(), WorkspaceConfig { disk_size_mib: 64 });
    store.ensure_layout().await.expect("layout");
    let lease = store
        .prepare_workspace(WORKSPACE_ID)
        .await
        .expect("workspace lease");

    write_marker(&lease.disk_path, b"seed");
    let checkpoint_a = store
        .create_checkpoint(&lease, Vec::new(), Some("a".into()), BTreeMap::new())
        .await
        .expect("checkpoint a");
    assert_eq!(checkpoint_a.source_checkpoint_id, None);

    sleep(Duration::from_millis(5)).await;

    write_marker(&lease.disk_path, b"branch-b");
    let checkpoint_b = store
        .create_checkpoint(&lease, Vec::new(), Some("b".into()), BTreeMap::new())
        .await
        .expect("checkpoint b");
    assert_eq!(
        checkpoint_b.source_checkpoint_id.as_deref(),
        Some(checkpoint_a.summary.checkpoint_id.as_str())
    );

    write_marker(&lease.disk_path, b"live");
    let restored = store
        .restore_checkpoint(
            WORKSPACE_ID,
            &checkpoint_a.summary.checkpoint_id,
            CheckpointRestoreMode::Rollback,
        )
        .await
        .expect("rollback restore");

    assert_eq!(
        restored.summary.checkpoint_id,
        checkpoint_a.summary.checkpoint_id
    );
    assert_eq!(read_marker(&lease.disk_path, 4), b"seed");

    let checkpoints = store
        .list_checkpoints(WORKSPACE_ID)
        .await
        .expect("checkpoints after restore");
    assert_eq!(checkpoints.len(), 1);
    assert_eq!(
        checkpoints[0].summary.checkpoint_id,
        checkpoint_a.summary.checkpoint_id
    );

    write_marker(&lease.disk_path, b"post");
    let checkpoint_c = store
        .create_checkpoint(&lease, Vec::new(), Some("c".into()), BTreeMap::new())
        .await
        .expect("checkpoint c");
    assert_eq!(
        checkpoint_c.source_checkpoint_id.as_deref(),
        Some(checkpoint_a.summary.checkpoint_id.as_str())
    );
}

#[tokio::test]
async fn delete_head_rolls_back_to_parent_lineage() {
    let temp = tempdir().expect("tempdir");
    let store = WorkspaceStore::new(temp.path(), WorkspaceConfig { disk_size_mib: 64 });
    store.ensure_layout().await.expect("layout");
    let lease = store
        .prepare_workspace(WORKSPACE_ID)
        .await
        .expect("workspace lease");

    write_marker(&lease.disk_path, b"seed");
    let checkpoint_a = store
        .create_checkpoint(&lease, Vec::new(), Some("a".into()), BTreeMap::new())
        .await
        .expect("checkpoint a");

    sleep(Duration::from_millis(5)).await;

    write_marker(&lease.disk_path, b"branch");
    let checkpoint_b = store
        .create_checkpoint(&lease, Vec::new(), Some("b".into()), BTreeMap::new())
        .await
        .expect("checkpoint b");

    store
        .delete_checkpoint(WORKSPACE_ID, &checkpoint_b.summary.checkpoint_id)
        .await
        .expect("delete head checkpoint");

    write_marker(&lease.disk_path, b"after");
    let checkpoint_c = store
        .create_checkpoint(&lease, Vec::new(), Some("c".into()), BTreeMap::new())
        .await
        .expect("checkpoint c");
    assert_eq!(
        checkpoint_c.source_checkpoint_id.as_deref(),
        Some(checkpoint_a.summary.checkpoint_id.as_str())
    );
}

#[tokio::test]
async fn replace_keeps_newer_checkpoints() {
    let temp = tempdir().expect("tempdir");
    let store = WorkspaceStore::new(temp.path(), WorkspaceConfig { disk_size_mib: 64 });
    store.ensure_layout().await.expect("layout");
    let lease = store
        .prepare_workspace(WORKSPACE_ID)
        .await
        .expect("workspace lease");

    write_marker(&lease.disk_path, b"seed");
    let checkpoint_a = store
        .create_checkpoint(&lease, Vec::new(), Some("a".into()), BTreeMap::new())
        .await
        .expect("checkpoint a");

    sleep(Duration::from_millis(5)).await;

    write_marker(&lease.disk_path, b"branch");
    let checkpoint_b = store
        .create_checkpoint(&lease, Vec::new(), Some("b".into()), BTreeMap::new())
        .await
        .expect("checkpoint b");

    write_marker(&lease.disk_path, b"live");
    let restored = store
        .restore_checkpoint(
            WORKSPACE_ID,
            &checkpoint_a.summary.checkpoint_id,
            CheckpointRestoreMode::Replace,
        )
        .await
        .expect("replace restore");

    assert_eq!(
        restored.summary.checkpoint_id,
        checkpoint_a.summary.checkpoint_id
    );
    assert_eq!(read_marker(&lease.disk_path, 4), b"seed");
    let checkpoints = store
        .list_checkpoints(WORKSPACE_ID)
        .await
        .expect("remaining checkpoints");
    assert_eq!(checkpoints.len(), 2);
    assert_eq!(
        checkpoints[0].summary.checkpoint_id,
        checkpoint_a.summary.checkpoint_id
    );
    assert_eq!(
        checkpoints[1].summary.checkpoint_id,
        checkpoint_b.summary.checkpoint_id
    );
}

#[tokio::test]
async fn fork_copies_checkpoint_without_mutating_source_workspace() {
    let temp = tempdir().expect("tempdir");
    let store = WorkspaceStore::new(temp.path(), WorkspaceConfig { disk_size_mib: 64 });
    store.ensure_layout().await.expect("layout");
    let source = store
        .prepare_workspace(SOURCE_WORKSPACE_ID)
        .await
        .expect("source workspace");

    write_marker(&source.disk_path, b"seed");
    let checkpoint = store
        .create_checkpoint(&source, Vec::new(), Some("seed".into()), BTreeMap::new())
        .await
        .expect("seed checkpoint");

    write_marker(&source.disk_path, b"live");
    store
        .fork_workspace(
            SOURCE_WORKSPACE_ID,
            &checkpoint.summary.checkpoint_id,
            FORK_WORKSPACE_ID,
        )
        .await
        .expect("fork workspace");

    let fork = store
        .prepare_workspace(FORK_WORKSPACE_ID)
        .await
        .expect("fork workspace lease");
    assert_eq!(read_marker(&source.disk_path, 4), b"live");
    assert_eq!(read_marker(&fork.disk_path, 4), b"seed");

    write_marker(&fork.disk_path, b"fork");
    let forked_checkpoint = store
        .create_checkpoint(&fork, Vec::new(), Some("fork".into()), BTreeMap::new())
        .await
        .expect("fork checkpoint");
    assert_eq!(forked_checkpoint.source_checkpoint_id, None);
}

fn write_marker(path: &Path, marker: &[u8]) {
    let mut file = OpenOptions::new()
        .write(true)
        .open(path)
        .expect("open workspace disk");
    file.seek(SeekFrom::Start(0)).expect("seek workspace disk");
    file.write_all(marker).expect("write marker");
    file.flush().expect("flush marker");
}

fn read_marker(path: &Path, len: usize) -> Vec<u8> {
    let mut file = OpenOptions::new()
        .read(true)
        .open(path)
        .expect("open workspace disk");
    let mut buffer = vec![0; len];
    file.seek(SeekFrom::Start(0)).expect("seek workspace disk");
    file.read_exact(&mut buffer).expect("read marker");
    buffer
}
