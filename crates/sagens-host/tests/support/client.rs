use uuid::Uuid;

use sagens_host::box_api::InteractiveTarget;
use sagens_host::config::IsolationMode;

use super::spawn::spawn_client_impl;

pub async fn spawn_client() -> sagens_host::BoxApiClient {
    spawn_client_impl(IsolationMode::Compat).await
}

pub async fn spawn_secure_client() -> sagens_host::BoxApiClient {
    spawn_client_impl(IsolationMode::Secure).await
}

pub async fn create_box(client: &sagens_host::BoxApiClient) -> Uuid {
    client.create_box().await.expect("create box").box_id
}

pub async fn start_box(client: &sagens_host::BoxApiClient, box_id: Uuid) {
    client.start_box(box_id).await.expect("start box");
}

pub async fn open_shell(client: &sagens_host::BoxApiClient, box_id: Uuid) -> sagens_host::BoxShell {
    client
        .open_shell(box_id, InteractiveTarget::Bash)
        .await
        .expect("open shell")
}
