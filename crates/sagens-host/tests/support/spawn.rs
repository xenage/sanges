use std::sync::Arc;

use async_trait::async_trait;
use sagens_host::auth::{AdminCredential, AdminStore, BoxCredentialStore, UserConfig};
use sagens_host::backend::ShellDriver;
use sagens_host::boxes::BoxManager;
use sagens_host::config::IsolationMode;
use sagens_host::protocol::ShellEvent;
use sagens_host::serve_box_api_websocket;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::support::service::StubBoxManager;

pub(super) struct StubShellDriver {
    pub(super) sender: mpsc::Sender<ShellEvent>,
}

#[async_trait]
impl ShellDriver for StubShellDriver {
    async fn send_input(&self, _: Uuid, data: Vec<u8>) -> sagens_host::Result<()> {
        let text = String::from_utf8_lossy(&data);
        if text.contains("shell-ok") || text.contains("ping") {
            let _ = self
                .sender
                .send(ShellEvent::Output(b"shell-ok\n".to_vec()))
                .await;
        }
        if text.contains("exit") || text.contains('\u{4}') {
            let _ = self.sender.send(ShellEvent::Exit(0)).await;
        }
        Ok(())
    }

    async fn resize(&self, _: Uuid, _: u16, _: u16) -> sagens_host::Result<()> {
        Ok(())
    }

    async fn close(&self, _: Uuid) -> sagens_host::Result<()> {
        let _ = self.sender.send(ShellEvent::Exit(0)).await;
        Ok(())
    }
}

pub(crate) async fn spawn_client_impl(isolation_mode: IsolationMode) -> sagens_host::BoxApiClient {
    let service: Arc<dyn BoxManager> = Arc::new(StubBoxManager::default());
    let state_dir = tempfile::tempdir().expect("tempdir").keep();
    let admin_store = Arc::new(AdminStore::new(&state_dir));
    let box_credential_store = Arc::new(BoxCredentialStore::new(&state_dir));
    let admin = AdminCredential {
        admin_uuid: Uuid::new_v4(),
        admin_token: "test-admin-token".into(),
    };
    admin_store.bootstrap(&admin).await.expect("bootstrap");
    let server = serve_box_api_websocket(
        "127.0.0.1:0".parse().expect("addr"),
        service,
        admin_store,
        box_credential_store,
        isolation_mode,
    )
    .await
    .expect("server");
    let config = UserConfig {
        version: 1,
        admin_uuid: admin.admin_uuid,
        admin_token: admin.admin_token,
        endpoint: format!("ws://{}", server.addr),
    };
    sagens_host::BoxApiClient::connect(&config)
        .await
        .expect("client")
}
