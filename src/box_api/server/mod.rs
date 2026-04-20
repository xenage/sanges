mod checkpoints;
mod dispatch;
mod execution;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio::sync::{Mutex, watch};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;

use crate::auth::{AdminStore, BoxCredentialStore};
use crate::backend::ShellHandle;
use crate::boxes::BoxManager;
use crate::config::IsolationMode;
use crate::{Result, SandboxError};

use super::protocol::{BoxEvent, ClientMessage, Principal, ServerMessage};

pub(super) type WsWriter = Arc<
    Mutex<
        futures_util::stream::SplitSink<
            tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
            Message,
        >,
    >,
>;
#[derive(Clone)]
pub(super) struct ShellSessionEntry {
    pub box_id: Uuid,
    pub handle: ShellHandle,
}

pub(super) type ShellSessions = Arc<Mutex<HashMap<Uuid, ShellSessionEntry>>>;

#[derive(Clone)]
struct ConnectionContext {
    service: Arc<dyn BoxManager>,
    admin_store: Arc<AdminStore>,
    box_credential_store: Arc<BoxCredentialStore>,
    isolation_mode: IsolationMode,
    endpoint: String,
    shutdown_tx: watch::Sender<bool>,
    shutdown_rx: watch::Receiver<bool>,
}

pub struct BoxApiServerHandle {
    pub addr: SocketAddr,
    shutdown: watch::Sender<bool>,
    task: JoinHandle<Result<()>>,
}

impl BoxApiServerHandle {
    pub fn shutdown(&self) {
        let _ = self.shutdown.send(true);
    }

    pub(crate) fn shutdown_signal(&self) -> watch::Sender<bool> {
        self.shutdown.clone()
    }

    pub async fn wait(self) -> Result<()> {
        self.task.await.map_err(|error| {
            SandboxError::backend(format!("box api server task join error: {error}"))
        })?
    }
}

pub async fn serve_box_api_websocket(
    bind_addr: SocketAddr,
    service: Arc<dyn BoxManager>,
    admin_store: Arc<AdminStore>,
    box_credential_store: Arc<BoxCredentialStore>,
    isolation_mode: IsolationMode,
) -> Result<BoxApiServerHandle> {
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let listener = TcpListener::bind(bind_addr)
        .await
        .map_err(|error| SandboxError::io("binding box api websocket listener", error))?;
    let addr = listener
        .local_addr()
        .map_err(|error| SandboxError::io("reading box api listener address", error))?;
    let endpoint = format!("ws://{addr}");
    let handle_shutdown = shutdown_tx.clone();
    let task = tokio::spawn(async move {
        let mut shutdown_rx = shutdown_rx;
        loop {
            tokio::select! {
                changed = shutdown_rx.changed() => {
                    if changed.is_ok() && *shutdown_rx.borrow() {
                        break;
                    }
                }
                accepted = listener.accept() => {
                    let (stream, _) = accepted
                        .map_err(|error| SandboxError::io("accepting box api connection", error))?;
                    let context = ConnectionContext {
                        service: service.clone(),
                        admin_store: admin_store.clone(),
                        box_credential_store: box_credential_store.clone(),
                        isolation_mode,
                        endpoint: endpoint.clone(),
                        shutdown_tx: shutdown_tx.clone(),
                        shutdown_rx: shutdown_rx.clone(),
                    };
                    tokio::spawn(async move {
                        if let Err(error) = handle_connection(stream, context).await {
                            eprintln!("box api connection failed: {error}");
                        }
                    });
                }
            }
        }
        Ok(())
    });
    Ok(BoxApiServerHandle {
        addr,
        shutdown: handle_shutdown,
        task,
    })
}

async fn handle_connection(
    stream: tokio::net::TcpStream,
    context: ConnectionContext,
) -> Result<()> {
    let websocket = tokio_tungstenite::accept_async(stream)
        .await
        .map_err(|error| SandboxError::backend(format!("accepting box api websocket: {error}")))?;
    let (write, mut read) = websocket.split();
    let writer: WsWriter = Arc::new(Mutex::new(write));
    let shells: ShellSessions = Arc::new(Mutex::new(HashMap::new()));
    let mut principal = None;
    let mut shutdown_rx = context.shutdown_rx.clone();

    loop {
        let message = tokio::select! {
            changed = shutdown_rx.changed() => {
                if changed.is_ok() && *shutdown_rx.borrow() {
                    break;
                }
                continue;
            }
            message = read.next() => message,
        };
        let Some(message) = message else {
            break;
        };
        match message
            .map_err(|error| SandboxError::backend(format!("reading box api websocket: {error}")))?
        {
            Message::Text(text) => {
                let message: ClientMessage = serde_json::from_str(&text)
                    .map_err(|error| SandboxError::json("decoding box api request", error))?;
                let should_close = match message {
                    ClientMessage::AuthenticateAdmin {
                        admin_uuid,
                        admin_token,
                    } => {
                        authenticate_admin(
                            &writer,
                            &context.admin_store,
                            &mut principal,
                            admin_uuid,
                            admin_token,
                        )
                        .await?
                    }
                    ClientMessage::AuthenticateBox { box_id, box_token } => {
                        authenticate_box(
                            &writer,
                            &context.box_credential_store,
                            &mut principal,
                            context.isolation_mode,
                            box_id,
                            box_token,
                        )
                        .await?
                    }
                    ClientMessage::Request { request } => {
                        let request_id = request.request_id().to_string();
                        let current_principal = match principal.clone() {
                            Some(value) => value,
                            None => {
                                send_event(
                                    &writer,
                                    &BoxEvent::Error {
                                        request_id: Some(request_id),
                                        message: "authentication required".into(),
                                    },
                                )
                                .await?;
                                continue;
                            }
                        };
                        match dispatch::dispatch_request(
                            dispatch::DispatchContext {
                                service: context.service.clone(),
                                admin_store: context.admin_store.clone(),
                                box_credential_store: context.box_credential_store.clone(),
                                writer: writer.clone(),
                                shells: shells.clone(),
                                endpoint: context.endpoint.clone(),
                            },
                            request,
                            current_principal,
                        )
                        .await
                        {
                            Ok(dispatch::ConnectionAction::KeepOpen) => false,
                            Ok(dispatch::ConnectionAction::Close) => true,
                            Ok(dispatch::ConnectionAction::ShutdownServer) => {
                                let _ = context.shutdown_tx.send(true);
                                true
                            }
                            Err(error) => {
                                send_event(
                                    &writer,
                                    &BoxEvent::Error {
                                        request_id: Some(request_id),
                                        message: error.to_string(),
                                    },
                                )
                                .await?;
                                false
                            }
                        }
                    }
                };
                if should_close {
                    break;
                }
            }
            Message::Close(_) => break,
            Message::Binary(_) | Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => {}
        }
    }

    for entry in shells.lock().await.drain().map(|(_, entry)| entry) {
        let _ = entry.handle.close().await;
    }
    Ok(())
}

async fn authenticate_admin(
    writer: &WsWriter,
    admin_store: &AdminStore,
    principal: &mut Option<Principal>,
    admin_uuid: Uuid,
    admin_token: String,
) -> Result<bool> {
    if admin_store.authenticate(admin_uuid, &admin_token).await? {
        let authenticated = Principal::Admin { admin_uuid };
        *principal = Some(authenticated.clone());
        send_server_message(
            writer,
            &ServerMessage::Authenticated {
                principal: authenticated,
            },
        )
        .await?;
        Ok(false)
    } else {
        send_event(
            writer,
            &BoxEvent::Error {
                request_id: None,
                message: "authentication failed".into(),
            },
        )
        .await?;
        Ok(true)
    }
}

async fn authenticate_box(
    writer: &WsWriter,
    box_credential_store: &BoxCredentialStore,
    principal: &mut Option<Principal>,
    isolation_mode: IsolationMode,
    box_id: Uuid,
    box_token: Option<String>,
) -> Result<bool> {
    if isolation_mode == IsolationMode::Secure {
        let Some(token) = box_token else {
            send_event(
                writer,
                &BoxEvent::Error {
                    request_id: None,
                    message: "box authentication requires a box_token in secure mode".into(),
                },
            )
            .await?;
            return Ok(true);
        };
        if !box_credential_store.authenticate(box_id, &token).await? {
            send_event(
                writer,
                &BoxEvent::Error {
                    request_id: None,
                    message: "authentication failed".into(),
                },
            )
            .await?;
            return Ok(true);
        }
    }
    let authenticated = Principal::Box { box_id };
    *principal = Some(authenticated.clone());
    send_server_message(
        writer,
        &ServerMessage::Authenticated {
            principal: authenticated,
        },
    )
    .await?;
    Ok(false)
}

pub(super) async fn send_event(writer: &WsWriter, event: &BoxEvent) -> Result<()> {
    send_server_message(
        writer,
        &ServerMessage::Event {
            event: Box::new(event.clone()),
        },
    )
    .await
}

pub(super) async fn send_server_message(writer: &WsWriter, message: &ServerMessage) -> Result<()> {
    let payload = serde_json::to_string(message)
        .map_err(|error| SandboxError::json("encoding box api server message", error))?;
    writer
        .lock()
        .await
        .send(Message::Text(payload.into()))
        .await
        .map_err(|error| SandboxError::backend(format!("sending box api message: {error}")))
}
