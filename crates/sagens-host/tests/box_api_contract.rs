mod support;

use sagens_host::ExecExit;

use support::{create_box, spawn_client, start_box};

#[tokio::test]
async fn websocket_contract_preserves_exec_and_file_flow() {
    let client = spawn_client().await;
    let box_id = create_box(&client).await;
    start_box(&client, box_id).await;

    let exec = client
        .exec_bash_capture(box_id, "touch tracked.txt".into())
        .await
        .expect("exec");
    assert_eq!(exec.exit_status, ExecExit::Success);
    let files = client
        .list_files(box_id, "/workspace".into())
        .await
        .expect("files");
    assert!(files.iter().any(|entry| entry.path == "tracked.txt"));

    client
        .write_file(
            box_id,
            "/workspace/tracked.txt".into(),
            b"hello websocket".to_vec(),
            true,
        )
        .await
        .expect("write");
    let file = client
        .read_file(box_id, "/workspace/tracked.txt".into(), 4096)
        .await
        .expect("read");
    assert_eq!(String::from_utf8_lossy(&file.data), "hello websocket");
}
