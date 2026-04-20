use base64::Engine as _;
use uuid::Uuid;

use super::dispatch::authorize_box;
use super::{ShellSessionEntry, ShellSessions, WsWriter, send_event};
use crate::box_api::protocol::{BoxEvent, InteractiveTarget, Principal};
use crate::protocol::{CommandStream, ExecRequest, ExecutionEvent};
use crate::{Result, SandboxError};

pub(super) fn python_exec(
    args: Vec<String>,
    timeout_ms: Option<u64>,
    kill_grace_ms: Option<u64>,
) -> ExecRequest {
    ExecRequest {
        program: "/usr/bin/env".into(),
        args: std::iter::once("python3".into()).chain(args).collect(),
        cwd: "/workspace".into(),
        env: std::collections::BTreeMap::new(),
        timeout_ms,
        kill_grace_ms: kill_grace_ms.unwrap_or(250),
    }
}

pub(super) fn shell_request(target: InteractiveTarget) -> crate::protocol::ShellRequest {
    match target {
        InteractiveTarget::Bash => crate::protocol::ShellRequest::default(),
        InteractiveTarget::Python => crate::protocol::ShellRequest {
            program: "/usr/bin/env".into(),
            args: vec!["python3".into(), "-i".into()],
            cwd: "/workspace".into(),
            env: std::collections::BTreeMap::from([
                ("TERM".into(), "dumb".into()),
                ("PYTHONUNBUFFERED".into(), "1".into()),
            ]),
        },
    }
}

pub(super) async fn shell_handle(
    shells: &ShellSessions,
    principal: &Principal,
    shell_id: Uuid,
) -> Result<ShellSessionEntry> {
    let entry = shells
        .lock()
        .await
        .get(&shell_id)
        .cloned()
        .ok_or_else(|| SandboxError::invalid(format!("unknown shell session {shell_id}")))?;
    authorize_box(principal, entry.box_id)?;
    Ok(entry)
}

pub(super) async fn remove_shell(
    shells: &ShellSessions,
    principal: &Principal,
    shell_id: Uuid,
) -> Result<Option<ShellSessionEntry>> {
    let mut shells = shells.lock().await;
    if let Some(entry) = shells.get(&shell_id) {
        authorize_box(principal, entry.box_id)?;
    }
    Ok(shells.remove(&shell_id))
}

pub(super) fn spawn_exec_stream(writer: WsWriter, request_id: String, mut stream: CommandStream) {
    tokio::spawn(async move {
        while let Some(event) = stream.next().await {
            match event {
                ExecutionEvent::Output { stream, data } => {
                    if send_event(
                        &writer,
                        &BoxEvent::ExecOutput {
                            request_id: request_id.clone(),
                            stream,
                            data: base64::engine::general_purpose::STANDARD.encode(data),
                        },
                    )
                    .await
                    .is_err()
                    {
                        break;
                    }
                }
                ExecutionEvent::Exit { status } => {
                    let _ = send_event(
                        &writer,
                        &BoxEvent::ExecExit {
                            request_id: request_id.clone(),
                            status,
                        },
                    )
                    .await;
                    break;
                }
            }
        }
    });
}

pub(super) async fn send_shell_event(
    writer: &WsWriter,
    shell_id: Uuid,
    bytes: Vec<u8>,
) -> Result<()> {
    send_event(
        writer,
        &BoxEvent::ShellOutput {
            shell_id,
            data: base64::engine::general_purpose::STANDARD.encode(bytes),
        },
    )
    .await
}

pub(super) fn decode_bytes(context: &str, value: &str) -> Result<Vec<u8>> {
    base64::engine::general_purpose::STANDARD
        .decode(value)
        .map_err(|error| SandboxError::protocol(format!("{context}: {error}")))
}
