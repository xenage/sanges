mod support;

use std::collections::BTreeMap;

use sagens_host::CheckpointRestoreMode;
use uuid::Uuid;

use support::{create_box, spawn_client, start_box};

async fn read_text(client: &sagens_host::BoxApiClient, box_id: Uuid, path: &str) -> String {
    let file = client
        .read_file(box_id, path.into(), 8 * 1024)
        .await
        .expect("read file");
    String::from_utf8(file.data).expect("utf8 file")
}

#[tokio::test]
async fn websocket_supports_checkpoint_lineage_flow() {
    let client = spawn_client().await;
    let box_id = create_box(&client).await;
    start_box(&client, box_id).await;

    client
        .write_file(
            box_id,
            "/workspace/notes.txt".into(),
            b"hello checkpoint".to_vec(),
            true,
        )
        .await
        .expect("write seed file");

    let seed = client
        .checkpoint_create(
            box_id,
            Some("seed".into()),
            BTreeMap::from([("purpose".into(), "test".into())]),
        )
        .await
        .expect("create seed checkpoint");
    assert_eq!(seed.summary.workspace_id, box_id.to_string());
    assert_eq!(seed.summary.name.as_deref(), Some("seed"));
    assert_eq!(seed.source_checkpoint_id, None);
    assert_eq!(
        seed.summary.metadata.get("purpose").map(String::as_str),
        Some("test")
    );

    client
        .write_file(
            box_id,
            "/workspace/notes.txt".into(),
            b"second version".to_vec(),
            true,
        )
        .await
        .expect("write second file");
    let second = client
        .checkpoint_create(box_id, Some("second".into()), BTreeMap::new())
        .await
        .expect("create second checkpoint");
    assert_eq!(
        second.source_checkpoint_id.as_deref(),
        Some(seed.summary.checkpoint_id.as_str())
    );

    let checkpoints = client
        .checkpoint_list(box_id)
        .await
        .expect("list checkpoints");
    assert_eq!(checkpoints.len(), 2);
    assert_eq!(checkpoints[0].source_checkpoint_id, None);
    assert_eq!(
        checkpoints[1].source_checkpoint_id.as_deref(),
        Some(seed.summary.checkpoint_id.as_str())
    );

    let restored = client
        .checkpoint_restore(
            box_id,
            seed.summary.checkpoint_id.clone(),
            CheckpointRestoreMode::Rollback,
        )
        .await
        .expect("restore checkpoint");
    assert_eq!(restored.summary.checkpoint_id, seed.summary.checkpoint_id);
    assert_eq!(
        read_text(&client, box_id, "/workspace/notes.txt").await,
        "hello checkpoint"
    );

    client
        .write_file(
            box_id,
            "/workspace/notes.txt".into(),
            b"after restore".to_vec(),
            true,
        )
        .await
        .expect("write post-restore file");
    let after_restore = client
        .checkpoint_create(box_id, Some("after-restore".into()), BTreeMap::new())
        .await
        .expect("create post-restore checkpoint");
    assert_eq!(
        after_restore.source_checkpoint_id.as_deref(),
        Some(seed.summary.checkpoint_id.as_str())
    );

    client
        .write_file(
            box_id,
            "/workspace/notes.txt".into(),
            b"source must stay mutated".to_vec(),
            true,
        )
        .await
        .expect("write source mutation");

    let forked = client
        .checkpoint_fork(
            box_id,
            seed.summary.checkpoint_id.clone(),
            Some("forked-box".into()),
        )
        .await
        .expect("fork checkpoint");
    assert_eq!(forked.name.as_deref(), Some("forked-box"));
    assert_ne!(forked.box_id, box_id);

    client.stop_box(box_id).await.expect("stop source box");
    start_box(&client, box_id).await;
    assert_eq!(
        read_text(&client, box_id, "/workspace/notes.txt").await,
        "source must stay mutated"
    );

    start_box(&client, forked.box_id).await;
    assert_eq!(
        read_text(&client, forked.box_id, "/workspace/notes.txt").await,
        "hello checkpoint"
    );

    client
        .write_file(
            forked.box_id,
            "/workspace/notes.txt".into(),
            b"forked version".to_vec(),
            true,
        )
        .await
        .expect("write forked file");
    let forked_head = client
        .checkpoint_create(forked.box_id, Some("forked-head".into()), BTreeMap::new())
        .await
        .expect("create forked checkpoint");
    assert_eq!(forked_head.source_checkpoint_id, None);

    client
        .checkpoint_delete(box_id, after_restore.summary.checkpoint_id.clone())
        .await
        .expect("delete post-restore checkpoint");
    client
        .checkpoint_delete(box_id, second.summary.checkpoint_id.clone())
        .await
        .expect("delete second checkpoint");
    client
        .checkpoint_delete(box_id, seed.summary.checkpoint_id.clone())
        .await
        .expect("delete seed checkpoint");
    let checkpoints = client
        .checkpoint_list(box_id)
        .await
        .expect("list after delete");
    assert!(checkpoints.is_empty());

    client
        .stop_box(forked.box_id)
        .await
        .expect("stop forked box");
    client
        .remove_box(forked.box_id)
        .await
        .expect("remove forked box");
    client.remove_box(box_id).await.expect("remove source box");
}
