use uuid::Uuid;

use super::GuestRpcClient;
use crate::Result;
use crate::backend::ShellDriver;
use crate::guest_rpc::{GuestRequest, encode_bytes};

pub(super) struct RpcShellDriver {
    pub(super) client: GuestRpcClient,
}

#[async_trait::async_trait]
impl ShellDriver for RpcShellDriver {
    async fn send_input(&self, session_id: Uuid, data: Vec<u8>) -> Result<()> {
        self.client
            .send_ack(GuestRequest::ShellInput {
                request_id: self.client.next_request_id(),
                session_id,
                data: encode_bytes(&data),
            })
            .await
    }

    async fn resize(&self, session_id: Uuid, cols: u16, rows: u16) -> Result<()> {
        self.client
            .send_ack(GuestRequest::ResizeShell {
                request_id: self.client.next_request_id(),
                session_id,
                cols,
                rows,
            })
            .await
    }

    async fn close(&self, session_id: Uuid) -> Result<()> {
        self.client
            .send_ack(GuestRequest::CloseShell {
                request_id: self.client.next_request_id(),
                session_id,
            })
            .await
    }
}
