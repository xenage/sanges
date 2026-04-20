mod support;

use std::collections::BTreeMap;

use base64::Engine as _;
use sagens_host::ExecExit;
use tokio::join;

use support::{create_box, open_shell, spawn_client, start_box};

#[tokio::test]
async fn websocket_supports_shell_commit_timeout_and_rejects_parallel_same_box_exec() {
    let client = spawn_client().await;
    let box_a = create_box(&client).await;
    let box_b = create_box(&client).await;
    start_box(&client, box_a).await;
    start_box(&client, box_b).await;

    let shell = open_shell(&client, box_a).await;
    shell
        .send_input(b"shell-ok\nexit\n".to_vec())
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
        .checkpoint_create(box_a, Some("shell-flow".into()), BTreeMap::new())
        .await
        .expect("checkpoint");
    assert_eq!(checkpoint.summary.workspace_id, box_a.to_string());
    assert_eq!(checkpoint.summary.name.as_deref(), Some("shell-flow"));

    let (first, second) = join!(
        client.exec_bash_capture(box_a, "sleep 0.15".into()),
        client.exec_bash_capture(box_a, "echo blocked".into()),
    );
    let first = first.expect("first exec");
    let second = second.expect_err("parallel exec should fail");
    assert_eq!(first.exit_status, ExecExit::Success);
    assert!(
        second
            .to_string()
            .contains("parallel exec is not supported"),
        "unexpected error: {second}"
    );
    assert!(String::from_utf8_lossy(&first.stdout).contains(&box_a.to_string()));

    let (first, second) = join!(
        client.exec_bash_capture(box_a, "sleep 0.15".into()),
        client.exec_bash_capture(box_b, "echo fast".into()),
    );
    let first = first.expect("first exec");
    let second = second.expect("second exec");
    assert_eq!(first.exit_status, ExecExit::Success);
    assert_eq!(second.exit_status, ExecExit::Success);
    assert_eq!(first.exit_code, Some(0));
    assert_eq!(second.exit_code, Some(0));
    assert!(String::from_utf8_lossy(&first.stdout).contains(&box_a.to_string()));
    assert!(String::from_utf8_lossy(&second.stdout).contains(&box_b.to_string()));
    assert!(String::from_utf8_lossy(&second.output).contains(&box_b.to_string()));

    let timeout = client
        .exec_bash_with_timeout(box_a, "infinite".into(), 10, 10)
        .await
        .expect("timeout exec");
    assert_eq!(timeout.exit_status, ExecExit::Timeout);
    assert_eq!(timeout.exit_code, None);

    let killed = client
        .exec_bash_with_timeout(box_b, "ignore-term".into(), 10, 10)
        .await
        .expect("killed exec");
    assert_eq!(killed.exit_status, ExecExit::Killed);
    assert_eq!(killed.exit_code, None);
}
