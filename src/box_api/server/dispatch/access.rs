use uuid::Uuid;

use crate::box_api::protocol::{BoxEvent, BoxResponse, Principal};
use crate::{Result, SandboxError};

use super::{WsWriter, send_event};

pub(crate) async fn send_response(
    writer: &WsWriter,
    request_id: String,
    response: BoxResponse,
) -> Result<()> {
    send_event(
        writer,
        &BoxEvent::Response {
            request_id,
            response: Box::new(response),
        },
    )
    .await
}

pub(crate) fn require_admin(principal: &Principal) -> Result<Uuid> {
    match principal {
        Principal::Admin { admin_uuid } => Ok(*admin_uuid),
        Principal::Box { .. } => Err(SandboxError::conflict(
            "admin authentication required for this websocket command",
        )),
    }
}

pub(crate) fn authorize_box(principal: &Principal, box_id: Uuid) -> Result<()> {
    match principal {
        Principal::Admin { .. } => Ok(()),
        Principal::Box {
            box_id: principal_box_id,
        } if *principal_box_id == box_id => Ok(()),
        Principal::Box {
            box_id: principal_box_id,
        } => Err(SandboxError::conflict(format!(
            "box-authenticated connection for {principal_box_id} cannot access BOX {box_id}"
        ))),
    }
}
