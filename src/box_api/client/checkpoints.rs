use std::collections::BTreeMap;

use crate::box_api::protocol::{BoxRequest, BoxResponse};
use crate::workspace::{CheckpointRestoreMode, WorkspaceCheckpointRecord};
use crate::{BoxRecord, Result};

use super::BoxApiClient;

impl BoxApiClient {
    pub async fn checkpoint_create(
        &self,
        box_id: uuid::Uuid,
        name: Option<String>,
        metadata: BTreeMap<String, String>,
    ) -> Result<WorkspaceCheckpointRecord> {
        let request_id = self.next_request_id();
        self.request_response(
            BoxRequest::CheckpointCreate {
                request_id,
                box_id,
                name,
                metadata,
            },
            |response| match response {
                BoxResponse::Checkpoint { checkpoint } => Some(checkpoint),
                _ => None,
            },
        )
        .await
    }

    pub async fn checkpoint_list(
        &self,
        box_id: uuid::Uuid,
    ) -> Result<Vec<WorkspaceCheckpointRecord>> {
        let request_id = self.next_request_id();
        self.request_response(
            BoxRequest::CheckpointList { request_id, box_id },
            |response| match response {
                BoxResponse::CheckpointList { checkpoints } => Some(checkpoints),
                _ => None,
            },
        )
        .await
    }

    pub async fn checkpoint_restore(
        &self,
        box_id: uuid::Uuid,
        checkpoint_id: String,
        mode: CheckpointRestoreMode,
    ) -> Result<WorkspaceCheckpointRecord> {
        let request_id = self.next_request_id();
        self.request_response(
            BoxRequest::CheckpointRestore {
                request_id,
                box_id,
                checkpoint_id,
                mode,
            },
            |response| match response {
                BoxResponse::Checkpoint { checkpoint } => Some(checkpoint),
                _ => None,
            },
        )
        .await
    }

    pub async fn checkpoint_fork(
        &self,
        box_id: uuid::Uuid,
        checkpoint_id: String,
        new_box_name: Option<String>,
    ) -> Result<BoxRecord> {
        let request_id = self.next_request_id();
        self.request_response(
            BoxRequest::CheckpointFork {
                request_id,
                box_id,
                checkpoint_id,
                new_box_name,
            },
            |response| match response {
                BoxResponse::Box { record } => Some(record),
                _ => None,
            },
        )
        .await
    }

    pub async fn checkpoint_delete(&self, box_id: uuid::Uuid, checkpoint_id: String) -> Result<()> {
        let request_id = self.next_request_id();
        self.request_ack(BoxRequest::CheckpointDelete {
            request_id,
            box_id,
            checkpoint_id,
        })
        .await
    }
}
