use base64::Engine as _;

use super::super::BoxApiClient;
use crate::Result;
use crate::box_api::protocol::{BoxRequest, BoxResponse};
use crate::workspace::{FileNode, ReadFileResult, WorkspaceChange};

impl BoxApiClient {
    pub async fn list_files(&self, box_id: uuid::Uuid, path: String) -> Result<Vec<FileNode>> {
        let request_id = self.next_request_id();
        self.request_response(
            BoxRequest::FsList {
                request_id,
                box_id,
                path,
            },
            |response| match response {
                BoxResponse::Files { entries, .. } => Some(entries),
                _ => None,
            },
        )
        .await
    }

    pub async fn read_file(
        &self,
        box_id: uuid::Uuid,
        path: String,
        limit: usize,
    ) -> Result<ReadFileResult> {
        let request_id = self.next_request_id();
        self.request_response(
            BoxRequest::FsRead {
                request_id,
                box_id,
                path,
                limit,
            },
            |response| match response {
                BoxResponse::File { file } => Some(file),
                _ => None,
            },
        )
        .await
    }

    pub async fn write_file(
        &self,
        box_id: uuid::Uuid,
        path: String,
        data: Vec<u8>,
        create_parents: bool,
    ) -> Result<()> {
        let request_id = self.next_request_id();
        self.request_ack(BoxRequest::FsWrite {
            request_id,
            box_id,
            path,
            data: base64::engine::general_purpose::STANDARD.encode(data),
            create_parents,
        })
        .await
    }

    pub async fn make_dir(&self, box_id: uuid::Uuid, path: String, recursive: bool) -> Result<()> {
        let request_id = self.next_request_id();
        self.request_ack(BoxRequest::FsMkdir {
            request_id,
            box_id,
            path,
            recursive,
        })
        .await
    }

    pub async fn remove_path(
        &self,
        box_id: uuid::Uuid,
        path: String,
        recursive: bool,
    ) -> Result<()> {
        let request_id = self.next_request_id();
        self.request_ack(BoxRequest::FsRemove {
            request_id,
            box_id,
            path,
            recursive,
        })
        .await
    }

    pub async fn list_changes(&self, box_id: uuid::Uuid) -> Result<Vec<WorkspaceChange>> {
        let request_id = self.next_request_id();
        self.request_response(
            BoxRequest::FsDiff { request_id, box_id },
            |response| match response {
                BoxResponse::Changes { changes } => Some(changes),
                _ => None,
            },
        )
        .await
    }
}
