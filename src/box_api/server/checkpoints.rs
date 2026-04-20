use std::sync::Arc;

use crate::Result;
use crate::boxes::BoxManager;

use super::WsWriter;
use super::dispatch::{authorize_box, send_response};
use crate::box_api::protocol::{BoxResponse, Principal};
use crate::workspace::CheckpointRestoreMode;

pub(super) async fn create_checkpoint(
    service: Arc<dyn BoxManager>,
    writer: &WsWriter,
    principal: &Principal,
    request_id: String,
    box_id: uuid::Uuid,
    name: Option<String>,
    metadata: std::collections::BTreeMap<String, String>,
) -> Result<()> {
    authorize_box(principal, box_id)?;
    let checkpoint = service.checkpoint_create(box_id, name, metadata).await?;
    send_response(writer, request_id, BoxResponse::Checkpoint { checkpoint }).await
}

pub(super) async fn list_checkpoints(
    service: Arc<dyn BoxManager>,
    writer: &WsWriter,
    principal: &Principal,
    request_id: String,
    box_id: uuid::Uuid,
) -> Result<()> {
    authorize_box(principal, box_id)?;
    let checkpoints = service.checkpoint_list(box_id).await?;
    send_response(
        writer,
        request_id,
        BoxResponse::CheckpointList { checkpoints },
    )
    .await
}

pub(super) async fn restore_checkpoint(
    service: Arc<dyn BoxManager>,
    writer: &WsWriter,
    principal: &Principal,
    request_id: String,
    box_id: uuid::Uuid,
    checkpoint_id: String,
    mode: CheckpointRestoreMode,
) -> Result<()> {
    authorize_box(principal, box_id)?;
    let checkpoint = service
        .checkpoint_restore(box_id, &checkpoint_id, mode)
        .await?;
    send_response(writer, request_id, BoxResponse::Checkpoint { checkpoint }).await
}

pub(super) async fn fork_checkpoint(
    service: Arc<dyn BoxManager>,
    writer: &WsWriter,
    principal: &Principal,
    request_id: String,
    box_id: uuid::Uuid,
    checkpoint_id: String,
    new_box_name: Option<String>,
) -> Result<()> {
    authorize_box(principal, box_id)?;
    let record = service
        .checkpoint_fork(box_id, &checkpoint_id, new_box_name)
        .await?;
    send_response(writer, request_id, BoxResponse::Box { record }).await
}

pub(super) async fn delete_checkpoint(
    service: Arc<dyn BoxManager>,
    writer: &WsWriter,
    principal: &Principal,
    request_id: String,
    box_id: uuid::Uuid,
    checkpoint_id: String,
) -> Result<()> {
    authorize_box(principal, box_id)?;
    service.checkpoint_delete(box_id, &checkpoint_id).await?;
    send_response(writer, request_id, BoxResponse::Ack).await
}
