use super::super::BoxApiClient;
use crate::Result;
use crate::box_api::protocol::{BoxRequest, BoxResponse};

impl BoxApiClient {
    pub async fn shutdown_daemon(&self) -> Result<()> {
        let request_id = self.next_request_id();
        self.request_ack(BoxRequest::ShutdownDaemon { request_id })
            .await
    }

    pub async fn admin_add(&self) -> Result<crate::auth::AdminCredentialBundle> {
        let request_id = self.next_request_id();
        self.request_response(
            BoxRequest::AdminAdd { request_id },
            |response| match response {
                BoxResponse::AdminAdded { bundle } => Some(bundle),
                _ => None,
            },
        )
        .await
    }

    pub async fn issue_box_credentials(
        &self,
        box_id: uuid::Uuid,
    ) -> Result<crate::auth::BoxCredentialBundle> {
        let request_id = self.next_request_id();
        self.request_response(
            BoxRequest::BoxIssueCredentials { request_id, box_id },
            |response| match response {
                BoxResponse::BoxCredentials { bundle } => Some(bundle),
                _ => None,
            },
        )
        .await
    }

    pub async fn admin_remove_me(&self) -> Result<()> {
        let request_id = self.next_request_id();
        self.request_ack(BoxRequest::AdminRemoveMe { request_id })
            .await
    }
}
