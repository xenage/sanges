use std::sync::Arc;

use async_trait::async_trait;
use sagens_host::backend::{ShellDriver, ShellSession};
use sagens_host::protocol::ShellEvent;
use tokio::sync::mpsc;
use uuid::Uuid;

pub(super) fn new_shell_session() -> ShellSession {
    let session_id = Uuid::new_v4();
    let (tx, rx) = mpsc::channel(8);
    ShellSession::new(session_id, rx, Arc::new(StubShellDriver { sender: tx }))
}

struct StubShellDriver {
    sender: mpsc::Sender<ShellEvent>,
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
