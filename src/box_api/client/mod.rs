mod checkpoints;
mod exec;
mod ops;
mod shell;
mod state;
mod terminal;
mod tty;

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use futures_util::{SinkExt, StreamExt};
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};
use uuid::Uuid;

use crate::auth::UserConfig;
use crate::{Result, SandboxError};

use super::protocol::{BoxEvent, BoxRequest, BoxResponse, ClientMessage, ServerMessage};
use state::{ClientInner, ClientState, validate_auth};

type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;
type Writer = futures_util::stream::SplitSink<Socket, Message>;

pub struct BoxApiClient {
    inner: Arc<ClientInner>,
    next_id: Arc<AtomicU64>,
    endpoint: String,
    auth: ClientMessage,
}

pub struct BoxShell {
    client: Arc<ClientInner>,
    next_id: Arc<AtomicU64>,
    shell_id: Uuid,
    events: Mutex<mpsc::Receiver<BoxEvent>>,
}

#[derive(Clone)]
pub(crate) struct BoxShellHandle {
    client: Arc<ClientInner>,
    next_id: Arc<AtomicU64>,
    shell_id: Uuid,
}

impl BoxApiClient {
    pub async fn connect(config: &UserConfig) -> Result<Self> {
        Self::connect_with_auth(
            &config.endpoint,
            ClientMessage::AuthenticateAdmin {
                admin_uuid: config.admin_uuid,
                admin_token: config.admin_token.clone(),
            },
        )
        .await
    }

    pub async fn connect_as_box(
        endpoint: &str,
        box_id: Uuid,
        box_token: Option<String>,
    ) -> Result<Self> {
        Self::connect_with_auth(
            endpoint,
            ClientMessage::AuthenticateBox { box_id, box_token },
        )
        .await
    }

    async fn connect_with_auth(endpoint: &str, auth: ClientMessage) -> Result<Self> {
        let auth_for_reader = auth.clone();
        let (socket, _) = connect_async(endpoint).await.map_err(|error| {
            SandboxError::backend(format!("connecting sagens websocket: {error}"))
        })?;
        let (writer, mut reader) = socket.split();
        let writer = Arc::new(Mutex::new(writer));
        let inner = Arc::new(ClientInner {
            writer: writer.clone(),
            state: Mutex::new(ClientState::default()),
        });
        let (auth_tx, auth_rx) = oneshot::channel();
        let reader_inner = inner.clone();

        tokio::spawn(async move {
            let mut auth_tx = Some(auth_tx);
            while let Some(message) = reader.next().await {
                match message {
                    Ok(Message::Text(text)) => match serde_json::from_str::<ServerMessage>(&text) {
                        Ok(ServerMessage::Authenticated { principal }) => {
                            if let Some(sender) = auth_tx.take() {
                                let _ = sender.send(validate_auth(principal, &auth_for_reader));
                            }
                        }
                        Ok(ServerMessage::Event { event }) => {
                            reader_inner.dispatch_event(*event).await;
                        }
                        Err(error) => {
                            let error =
                                SandboxError::json("decoding websocket server message", error);
                            if let Some(sender) = auth_tx.take() {
                                let _ = sender.send(Err(SandboxError::backend(error.to_string())));
                            }
                            reader_inner.fail_all(error.to_string()).await;
                            return;
                        }
                    },
                    Ok(Message::Close(_)) => break,
                    Ok(Message::Binary(_))
                    | Ok(Message::Ping(_))
                    | Ok(Message::Pong(_))
                    | Ok(Message::Frame(_)) => {}
                    Err(error) => {
                        let error = SandboxError::backend(format!(
                            "reading websocket message failed: {error}"
                        ));
                        if let Some(sender) = auth_tx.take() {
                            let _ = sender.send(Err(SandboxError::backend(error.to_string())));
                        }
                        reader_inner.fail_all(error.to_string()).await;
                        return;
                    }
                }
            }
            if let Some(sender) = auth_tx.take() {
                let _ = sender.send(Err(SandboxError::backend("websocket connection closed")));
            }
            reader_inner
                .fail_all("websocket connection closed".into())
                .await;
        });

        let payload = serde_json::to_string(&auth)
            .map_err(|error| SandboxError::json("encoding auth request", error))?;
        writer
            .lock()
            .await
            .send(Message::Text(payload.into()))
            .await
            .map_err(|error| SandboxError::backend(format!("sending auth request: {error}")))?;
        auth_rx
            .await
            .map_err(|_| SandboxError::backend("auth handshake channel closed"))??;

        Ok(Self {
            inner,
            next_id: Arc::new(AtomicU64::new(1)),
            endpoint: endpoint.into(),
            auth,
        })
    }

    pub fn next_request_id(&self) -> String {
        self.next_id.fetch_add(1, Ordering::Relaxed).to_string()
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    pub(super) async fn reconnect(&self) -> Result<Self> {
        Self::connect_with_auth(&self.endpoint, self.auth.clone()).await
    }

    async fn send_request(&self, request: BoxRequest) -> Result<()> {
        let payload = serde_json::to_string(&ClientMessage::Request { request })
            .map_err(|error| SandboxError::json("encoding websocket request", error))?;
        self.inner
            .writer
            .lock()
            .await
            .send(Message::Text(payload.into()))
            .await
            .map_err(|error| SandboxError::backend(format!("sending websocket request: {error}")))
    }

    async fn register_response(
        &self,
        request_id: String,
    ) -> oneshot::Receiver<Result<BoxResponse>> {
        let (tx, rx) = oneshot::channel();
        self.inner
            .state
            .lock()
            .await
            .pending_responses
            .insert(request_id, tx);
        rx
    }

    async fn request_box(&self, request: BoxRequest) -> Result<crate::BoxRecord> {
        self.request_response(request, |response| match response {
            BoxResponse::Box { record } => Some(record),
            _ => None,
        })
        .await
    }

    async fn request_ack(&self, request: BoxRequest) -> Result<()> {
        self.request_response(request, |response| match response {
            BoxResponse::Ack => Some(()),
            _ => None,
        })
        .await
    }

    async fn request_response<T>(
        &self,
        request: BoxRequest,
        mut map: impl FnMut(BoxResponse) -> Option<T>,
    ) -> Result<T> {
        let request_id = request.request_id().to_string();
        let receiver = self.register_response(request_id.clone()).await;
        self.send_request(request).await?;
        match receiver.await {
            Ok(Ok(response)) => map(response).ok_or_else(|| {
                SandboxError::protocol(format!("unexpected response type for request {request_id}"))
            }),
            Ok(Err(error)) => Err(error),
            Err(_) => Err(SandboxError::backend(format!(
                "response waiter dropped for request {request_id}"
            ))),
        }
    }

    async fn open_exec_channel(
        &self,
        request: BoxRequest,
    ) -> Result<(String, mpsc::Receiver<BoxEvent>)> {
        let request_id = request.request_id().to_string();
        let (tx, rx) = mpsc::channel(64);
        self.inner
            .state
            .lock()
            .await
            .exec_streams
            .insert(request_id.clone(), tx);
        if let Err(error) = self.send_request(request).await {
            self.inner
                .state
                .lock()
                .await
                .exec_streams
                .remove(&request_id);
            return Err(error);
        }
        Ok((request_id, rx))
    }

    async fn register_shell_channel(&self, shell_id: Uuid) -> mpsc::Receiver<BoxEvent> {
        let (tx, rx) = mpsc::channel(64);
        let buffered = {
            let mut state = self.inner.state.lock().await;
            state.shell_streams.insert(shell_id, tx.clone());
            state
                .buffered_shell_events
                .remove(&shell_id)
                .unwrap_or_default()
        };
        for event in buffered {
            let _ = tx.send(event).await;
        }
        rx
    }
}

fn decode_bytes(context: &str, value: &str) -> Result<Vec<u8>> {
    use base64::Engine as _;

    base64::engine::general_purpose::STANDARD
        .decode(value)
        .map_err(|error| SandboxError::protocol(format!("{context}: {error}")))
}
