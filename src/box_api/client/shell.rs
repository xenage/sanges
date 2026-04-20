use std::sync::atomic::Ordering;

use base64::Engine as _;
use futures_util::SinkExt;
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;

use crate::box_api::protocol::{BoxEvent, BoxRequest, ClientMessage};
use crate::{Result, SandboxError};

use tokio::sync::mpsc;

use super::{BoxShell, BoxShellHandle};

impl BoxShell {
    pub(crate) fn into_parts(self) -> (BoxShellHandle, mpsc::Receiver<BoxEvent>) {
        (
            BoxShellHandle {
                client: self.client,
                next_id: self.next_id,
                shell_id: self.shell_id,
            },
            self.events.into_inner(),
        )
    }

    pub fn shell_id(&self) -> Uuid {
        self.shell_id
    }

    pub async fn send_input(&self, data: Vec<u8>) -> Result<()> {
        self.send(BoxRequest::ShellInput {
            request_id: self.next_request_id(),
            shell_id: self.shell_id,
            data: base64::engine::general_purpose::STANDARD.encode(data),
        })
        .await
    }

    pub async fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        self.send(BoxRequest::ResizeShell {
            request_id: self.next_request_id(),
            shell_id: self.shell_id,
            cols,
            rows,
        })
        .await
    }

    pub async fn close(&self) -> Result<()> {
        self.send(BoxRequest::CloseShell {
            request_id: self.next_request_id(),
            shell_id: self.shell_id,
        })
        .await
    }

    pub async fn next_event(&self) -> Result<BoxEvent> {
        self.events
            .lock()
            .await
            .recv()
            .await
            .ok_or_else(|| SandboxError::backend("shell event channel closed"))
    }

    async fn send(&self, request: BoxRequest) -> Result<()> {
        let payload = serde_json::to_string(&ClientMessage::Request { request })
            .map_err(|error| SandboxError::json("encoding websocket request", error))?;
        self.client
            .writer
            .lock()
            .await
            .send(Message::Text(payload.into()))
            .await
            .map_err(|error| SandboxError::backend(format!("sending websocket request: {error}")))
    }

    fn next_request_id(&self) -> String {
        self.next_id.fetch_add(1, Ordering::Relaxed).to_string()
    }
}

impl BoxShellHandle {
    pub async fn send_input(&self, data: Vec<u8>) -> Result<()> {
        self.send(BoxRequest::ShellInput {
            request_id: self.next_request_id(),
            shell_id: self.shell_id,
            data: base64::engine::general_purpose::STANDARD.encode(data),
        })
        .await
    }

    pub async fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        self.send(BoxRequest::ResizeShell {
            request_id: self.next_request_id(),
            shell_id: self.shell_id,
            cols,
            rows,
        })
        .await
    }

    pub async fn close(&self) -> Result<()> {
        self.send(BoxRequest::CloseShell {
            request_id: self.next_request_id(),
            shell_id: self.shell_id,
        })
        .await
    }

    async fn send(&self, request: BoxRequest) -> Result<()> {
        let payload = serde_json::to_string(&ClientMessage::Request { request })
            .map_err(|error| SandboxError::json("encoding websocket request", error))?;
        self.client
            .writer
            .lock()
            .await
            .send(Message::Text(payload.into()))
            .await
            .map_err(|error| SandboxError::backend(format!("sending websocket request: {error}")))
    }

    fn next_request_id(&self) -> String {
        self.next_id.fetch_add(1, Ordering::Relaxed).to_string()
    }
}
