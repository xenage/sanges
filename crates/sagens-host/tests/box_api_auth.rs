mod support;

use std::collections::BTreeMap;

use base64::Engine as _;
use sagens_host::ExecExit;
use sagens_host::box_api::InteractiveTarget;

use support::{create_box, open_shell, spawn_client, spawn_secure_client, start_box};

#[tokio::test]
async fn websocket_serves_lifecycle_exec_shell_fs_and_box_scoped_auth() {
    let client = spawn_client().await;
    let box_id = create_box(&client).await;
    start_box(&client, box_id).await;

    let exec = client
        .exec_bash_capture(box_id, "echo local-ok".into())
        .await
        .expect("exec");
    assert_eq!(exec.exit_status, ExecExit::Success);
    assert!(String::from_utf8_lossy(&exec.stdout).contains("exec:"));

    client
        .write_file(
            box_id,
            "/workspace/tracked.txt".into(),
            b"hello".to_vec(),
            true,
        )
        .await
        .expect("write");
    let file = client
        .read_file(box_id, "/workspace/tracked.txt".into(), 4096)
        .await
        .expect("read");
    assert_eq!(file.data, b"hello");
    let files = client
        .list_files(box_id, "/workspace".into())
        .await
        .expect("files");
    assert!(files.iter().any(|entry| entry.path == "tracked.txt"));

    let shell = open_shell(&client, box_id).await;
    shell
        .send_input(b"ping\nexit\n".to_vec())
        .await
        .expect("shell input");
    let mut shell_output = String::new();
    loop {
        match shell.next_event().await.expect("shell event") {
            sagens_host::BoxEvent::ShellOutput { data, .. } => {
                shell_output.push_str(
                    &String::from_utf8(
                        base64::engine::general_purpose::STANDARD
                            .decode(data)
                            .expect("decode"),
                    )
                    .expect("utf-8"),
                );
            }
            sagens_host::BoxEvent::ShellExit { code, .. } => {
                assert_eq!(code, 0);
                break;
            }
            event => panic!("unexpected shell event: {event:?}"),
        }
    }
    assert!(shell_output.contains("shell-ok"));

    let checkpoint = client
        .checkpoint_create(box_id, Some("auth-flow".into()), BTreeMap::new())
        .await
        .expect("checkpoint");
    assert_eq!(checkpoint.summary.workspace_id, box_id.to_string());
    assert_eq!(checkpoint.summary.name.as_deref(), Some("auth-flow"));

    let bundle = client
        .issue_box_credentials(box_id)
        .await
        .expect("box credentials");
    let box_client = sagens_host::BoxApiClient::connect_as_box(
        client.endpoint(),
        box_id,
        Some(bundle.box_token),
    )
    .await
    .expect("box auth");
    assert!(box_client.list_boxes().await.is_err());
    let box_shell = box_client
        .open_shell(box_id, InteractiveTarget::Bash)
        .await
        .expect("box shell");
    box_shell
        .send_input(b"shell-ok\nexit\n".to_vec())
        .await
        .expect("box shell input");
    loop {
        if matches!(
            box_shell.next_event().await.expect("box shell event"),
            sagens_host::BoxEvent::ShellExit { code: 0, .. }
        ) {
            break;
        }
    }

    client.stop_box(box_id).await.expect("stop");
    client.remove_box(box_id).await.expect("remove");
}

#[tokio::test]
async fn secure_mode_rejects_uuid_only_box_auth_and_accepts_box_tokens() {
    let client = spawn_secure_client().await;
    let box_id = create_box(&client).await;

    let uuid_only =
        sagens_host::BoxApiClient::connect_as_box(client.endpoint(), box_id, None).await;
    assert!(uuid_only.is_err());

    let wrong_token = sagens_host::BoxApiClient::connect_as_box(
        client.endpoint(),
        box_id,
        Some("wrong-token".into()),
    )
    .await;
    assert!(wrong_token.is_err());

    let bundle = client
        .issue_box_credentials(box_id)
        .await
        .expect("box credentials");
    let box_client = sagens_host::BoxApiClient::connect_as_box(
        client.endpoint(),
        box_id,
        Some(bundle.box_token),
    )
    .await
    .expect("secure box auth");
    assert!(box_client.list_boxes().await.is_err());
}
