mod access;

use std::sync::Arc;

use super::execution::{
    decode_bytes, python_exec, remove_shell, send_shell_event, shell_handle, shell_request,
    spawn_exec_stream,
};
use super::{ShellSessionEntry, ShellSessions, WsWriter, checkpoints, send_event};
use crate::auth::{AdminStore, BoxCredentialStore};
use crate::box_api::protocol::{BoxEvent, BoxRequest, BoxResponse, Principal};
use crate::boxes::BoxManager;
use crate::protocol::{ExecRequest, ShellEvent};
use crate::{Result, SandboxError};

pub(crate) use access::{authorize_box, require_admin, send_response};

pub(super) enum ConnectionAction {
    KeepOpen,
    Close,
    ShutdownServer,
}

#[derive(Clone)]
pub(super) struct DispatchContext {
    pub service: Arc<dyn BoxManager>,
    pub admin_store: Arc<AdminStore>,
    pub box_credential_store: Arc<BoxCredentialStore>,
    pub writer: WsWriter,
    pub shells: ShellSessions,
    pub endpoint: String,
}

pub(super) async fn dispatch_request(
    context: DispatchContext,
    request: BoxRequest,
    principal: Principal,
) -> Result<ConnectionAction> {
    match request {
        BoxRequest::Ping { request_id } => {
            send_response(&context.writer, request_id, BoxResponse::Pong).await?;
        }
        BoxRequest::ListBoxes { request_id } => {
            require_admin(&principal)?;
            let boxes = context.service.list_boxes().await?;
            send_response(&context.writer, request_id, BoxResponse::BoxList { boxes }).await?;
        }
        BoxRequest::NewBox { request_id } => {
            require_admin(&principal)?;
            let record = context.service.create_box().await?;
            send_response(&context.writer, request_id, BoxResponse::Box { record }).await?;
        }
        BoxRequest::StartBox { request_id, box_id } => {
            authorize_box(&principal, box_id)?;
            let record = context.service.start_box(box_id).await?;
            send_response(&context.writer, request_id, BoxResponse::Box { record }).await?;
        }
        BoxRequest::StopBox { request_id, box_id } => {
            authorize_box(&principal, box_id)?;
            let record = context.service.stop_box(box_id).await?;
            send_response(&context.writer, request_id, BoxResponse::Box { record }).await?;
        }
        BoxRequest::RemoveBox { request_id, box_id } => {
            authorize_box(&principal, box_id)?;
            context.service.remove_box(box_id).await?;
            send_response(
                &context.writer,
                request_id,
                BoxResponse::BoxRemoved { box_id },
            )
            .await?;
        }
        BoxRequest::SetBoxSetting {
            request_id,
            box_id,
            value,
        } => {
            authorize_box(&principal, box_id)?;
            let record = context.service.set_box_setting(box_id, value).await?;
            send_response(&context.writer, request_id, BoxResponse::Box { record }).await?;
        }
        BoxRequest::ExecBash {
            request_id,
            box_id,
            command,
            timeout_ms,
            kill_grace_ms,
        } => {
            authorize_box(&principal, box_id)?;
            let mut request = ExecRequest::shell(command);
            request.timeout_ms = timeout_ms;
            if let Some(kill_grace_ms) = kill_grace_ms {
                request.kill_grace_ms = kill_grace_ms;
            }
            let stream = context.service.exec(box_id, request).await?;
            spawn_exec_stream(context.writer.clone(), request_id, stream);
        }
        BoxRequest::ExecPython {
            request_id,
            box_id,
            args,
            timeout_ms,
            kill_grace_ms,
        } => {
            authorize_box(&principal, box_id)?;
            let stream = context
                .service
                .exec(box_id, python_exec(args, timeout_ms, kill_grace_ms))
                .await?;
            spawn_exec_stream(context.writer.clone(), request_id, stream);
        }
        BoxRequest::OpenShell {
            request_id,
            box_id,
            target,
        } => {
            authorize_box(&principal, box_id)?;
            let session = context
                .service
                .open_shell(box_id, shell_request(target))
                .await?;
            let (handle, mut events) = session.into_parts();
            let shell_id = handle.id();
            context
                .shells
                .lock()
                .await
                .insert(shell_id, ShellSessionEntry { box_id, handle });
            let shell_writer = context.writer.clone();
            let shell_map = context.shells.clone();
            tokio::spawn(async move {
                while let Some(event) = events.recv().await {
                    match event {
                        ShellEvent::Started => {}
                        ShellEvent::Output(bytes) => {
                            if send_shell_event(&shell_writer, shell_id, bytes)
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                        ShellEvent::Exit(code) => {
                            shell_map.lock().await.remove(&shell_id);
                            let _ =
                                send_event(&shell_writer, &BoxEvent::ShellExit { shell_id, code })
                                    .await;
                            break;
                        }
                    }
                }
            });
            send_response(
                &context.writer,
                request_id,
                BoxResponse::ShellOpened { shell_id, box_id },
            )
            .await?;
        }
        BoxRequest::ShellInput {
            request_id,
            shell_id,
            data,
        } => {
            shell_handle(&context.shells, &principal, shell_id)
                .await?
                .handle
                .send_input(decode_bytes("invalid shell input payload", &data)?)
                .await?;
            send_response(&context.writer, request_id, BoxResponse::Ack).await?;
        }
        BoxRequest::ResizeShell {
            request_id,
            shell_id,
            cols,
            rows,
        } => {
            shell_handle(&context.shells, &principal, shell_id)
                .await?
                .handle
                .resize(cols, rows)
                .await?;
            send_response(&context.writer, request_id, BoxResponse::Ack).await?;
        }
        BoxRequest::CloseShell {
            request_id,
            shell_id,
        } => {
            if let Some(entry) = remove_shell(&context.shells, &principal, shell_id).await? {
                entry.handle.close().await?;
            }
            send_response(&context.writer, request_id, BoxResponse::Ack).await?;
        }
        BoxRequest::FsList {
            request_id,
            box_id,
            path,
        } => {
            authorize_box(&principal, box_id)?;
            let entries = context.service.list_files(box_id, &path).await?;
            send_response(
                &context.writer,
                request_id,
                BoxResponse::Files { path, entries },
            )
            .await?;
        }
        BoxRequest::FsRead {
            request_id,
            box_id,
            path,
            limit,
        } => {
            authorize_box(&principal, box_id)?;
            let file = context.service.read_file(box_id, &path, limit).await?;
            send_response(&context.writer, request_id, BoxResponse::File { file }).await?;
        }
        BoxRequest::FsWrite {
            request_id,
            box_id,
            path,
            data,
            create_parents,
        } => {
            authorize_box(&principal, box_id)?;
            context
                .service
                .write_file(
                    box_id,
                    &path,
                    &decode_bytes("invalid file payload", &data)?,
                    create_parents,
                )
                .await?;
            send_response(&context.writer, request_id, BoxResponse::Ack).await?;
        }
        BoxRequest::FsMkdir {
            request_id,
            box_id,
            path,
            recursive,
        } => {
            authorize_box(&principal, box_id)?;
            context.service.make_dir(box_id, &path, recursive).await?;
            send_response(&context.writer, request_id, BoxResponse::Ack).await?;
        }
        BoxRequest::FsRemove {
            request_id,
            box_id,
            path,
            recursive,
        } => {
            authorize_box(&principal, box_id)?;
            context
                .service
                .remove_path(box_id, &path, recursive)
                .await?;
            send_response(&context.writer, request_id, BoxResponse::Ack).await?;
        }
        BoxRequest::FsDiff { request_id, box_id } => {
            authorize_box(&principal, box_id)?;
            let changes = context.service.list_changes(box_id).await?;
            send_response(
                &context.writer,
                request_id,
                BoxResponse::Changes { changes },
            )
            .await?;
        }
        BoxRequest::CheckpointCreate {
            request_id,
            box_id,
            name,
            metadata,
        } => {
            checkpoints::create_checkpoint(
                context.service.clone(),
                &context.writer,
                &principal,
                request_id,
                box_id,
                name,
                metadata,
            )
            .await?;
        }
        BoxRequest::CheckpointList { request_id, box_id } => {
            checkpoints::list_checkpoints(
                context.service.clone(),
                &context.writer,
                &principal,
                request_id,
                box_id,
            )
            .await?;
        }
        BoxRequest::CheckpointRestore {
            request_id,
            box_id,
            checkpoint_id,
            mode,
        } => {
            checkpoints::restore_checkpoint(
                context.service.clone(),
                &context.writer,
                &principal,
                request_id,
                box_id,
                checkpoint_id,
                mode,
            )
            .await?;
        }
        BoxRequest::CheckpointFork {
            request_id,
            box_id,
            checkpoint_id,
            new_box_name,
        } => {
            checkpoints::fork_checkpoint(
                context.service.clone(),
                &context.writer,
                &principal,
                request_id,
                box_id,
                checkpoint_id,
                new_box_name,
            )
            .await?;
        }
        BoxRequest::CheckpointDelete {
            request_id,
            box_id,
            checkpoint_id,
        } => {
            checkpoints::delete_checkpoint(
                context.service.clone(),
                &context.writer,
                &principal,
                request_id,
                box_id,
                checkpoint_id,
            )
            .await?;
        }
        BoxRequest::ShutdownDaemon { request_id } => {
            require_admin(&principal)?;
            context.service.shutdown_daemon().await?;
            send_response(&context.writer, request_id, BoxResponse::Ack).await?;
            return Ok(ConnectionAction::ShutdownServer);
        }
        BoxRequest::AdminAdd { request_id } => {
            require_admin(&principal)?;
            let bundle = context
                .admin_store
                .add_admin(context.endpoint.clone())
                .await?;
            send_response(
                &context.writer,
                request_id,
                BoxResponse::AdminAdded { bundle },
            )
            .await?;
        }
        BoxRequest::BoxIssueCredentials { request_id, box_id } => {
            require_admin(&principal)?;
            let known_box = context
                .service
                .list_boxes()
                .await?
                .into_iter()
                .any(|record| record.box_id == box_id);
            if !known_box {
                return Err(SandboxError::not_found(format!("unknown BOX {box_id}")));
            }
            let bundle = context
                .box_credential_store
                .issue(context.endpoint.clone(), box_id)
                .await?;
            send_response(
                &context.writer,
                request_id,
                BoxResponse::BoxCredentials { bundle },
            )
            .await?;
        }
        BoxRequest::AdminRemoveMe { request_id } => {
            let admin_uuid = require_admin(&principal)?;
            context.admin_store.remove_admin(admin_uuid).await?;
            send_response(&context.writer, request_id, BoxResponse::Ack).await?;
            return Ok(ConnectionAction::Close);
        }
    }
    Ok(ConnectionAction::KeepOpen)
}
