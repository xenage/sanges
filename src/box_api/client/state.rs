use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{Mutex, mpsc, oneshot};
use uuid::Uuid;

use crate::box_api::protocol::{BoxEvent, BoxResponse, ClientMessage, Principal};
use crate::{Result, SandboxError};

use super::Writer;

type ResponseSender = oneshot::Sender<Result<BoxResponse>>;

#[derive(Default)]
pub(super) struct ClientState {
    pub(super) pending_responses: HashMap<String, ResponseSender>,
    pub(super) exec_streams: HashMap<String, mpsc::Sender<BoxEvent>>,
    pub(super) shell_streams: HashMap<Uuid, mpsc::Sender<BoxEvent>>,
    pub(super) buffered_shell_events: HashMap<Uuid, Vec<BoxEvent>>,
}

pub(super) struct ClientInner {
    pub(super) writer: Arc<Mutex<Writer>>,
    pub(super) state: Mutex<ClientState>,
}

impl ClientInner {
    pub(super) async fn dispatch_event(&self, event: BoxEvent) {
        match event {
            BoxEvent::Response {
                request_id,
                response,
            } => {
                if let Some(sender) = self
                    .state
                    .lock()
                    .await
                    .pending_responses
                    .remove(&request_id)
                {
                    let _ = sender.send(Ok(*response));
                }
            }
            BoxEvent::Error {
                request_id: Some(request_id),
                message,
            } => {
                let event = BoxEvent::Error {
                    request_id: Some(request_id.clone()),
                    message: message.clone(),
                };
                let mut state = self.state.lock().await;
                if let Some(sender) = state.pending_responses.remove(&request_id) {
                    let _ = sender.send(Err(SandboxError::backend(message)));
                    return;
                }
                if let Some(sender) = state.exec_streams.remove(&request_id) {
                    let _ = sender.send(event).await;
                }
            }
            BoxEvent::ExecOutput { ref request_id, .. } => {
                let sender = self
                    .state
                    .lock()
                    .await
                    .exec_streams
                    .get(request_id)
                    .cloned();
                if let Some(sender) = sender {
                    let _ = sender.send(event).await;
                }
            }
            BoxEvent::ExecExit { ref request_id, .. } => {
                let request_id = request_id.clone();
                let sender = self
                    .state
                    .lock()
                    .await
                    .exec_streams
                    .get(&request_id)
                    .cloned();
                if let Some(sender) = sender {
                    let _ = sender.send(event).await;
                }
                self.state.lock().await.exec_streams.remove(&request_id);
            }
            BoxEvent::ShellOutput { shell_id, .. } | BoxEvent::ShellExit { shell_id, .. } => {
                let is_terminal = matches!(event, BoxEvent::ShellExit { .. });
                let mut state = self.state.lock().await;
                if let Some(sender) = state.shell_streams.get(&shell_id).cloned() {
                    drop(state);
                    let _ = sender.send(event).await;
                    if is_terminal {
                        self.state.lock().await.shell_streams.remove(&shell_id);
                    }
                } else {
                    state
                        .buffered_shell_events
                        .entry(shell_id)
                        .or_default()
                        .push(event);
                }
            }
            BoxEvent::Error {
                request_id: None,
                message,
            } => {
                self.fail_all(message).await;
            }
        }
    }

    pub(super) async fn fail_all(&self, message: String) {
        let (responses, execs, shells) = {
            let mut state = self.state.lock().await;
            let responses = state
                .pending_responses
                .drain()
                .map(|(_, sender)| sender)
                .collect::<Vec<_>>();
            let execs = state
                .exec_streams
                .drain()
                .map(|(_, sender)| sender)
                .collect::<Vec<_>>();
            let shells = state
                .shell_streams
                .drain()
                .map(|(_, sender)| sender)
                .collect::<Vec<_>>();
            state.buffered_shell_events.clear();
            (responses, execs, shells)
        };
        for sender in responses {
            let _ = sender.send(Err(SandboxError::backend(message.clone())));
        }
        for sender in execs {
            let _ = sender
                .send(BoxEvent::Error {
                    request_id: None,
                    message: message.clone(),
                })
                .await;
        }
        for sender in shells {
            let _ = sender
                .send(BoxEvent::Error {
                    request_id: None,
                    message: message.clone(),
                })
                .await;
        }
    }
}

pub(super) fn validate_auth(principal: Principal, request: &ClientMessage) -> Result<()> {
    match (principal, request) {
        (
            Principal::Admin { admin_uuid: actual },
            ClientMessage::AuthenticateAdmin { admin_uuid, .. },
        ) if actual == *admin_uuid => Ok(()),
        (Principal::Box { box_id: actual }, ClientMessage::AuthenticateBox { box_id, .. })
            if actual == *box_id =>
        {
            Ok(())
        }
        _ => Err(SandboxError::backend(
            "websocket authenticated as an unexpected principal",
        )),
    }
}
