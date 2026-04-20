use tokio::io::{AsyncBufReadExt, BufReader};
use uuid::Uuid;

use super::{GuestResponse, GuestRpcClient};
use crate::guest_rpc::{GuestEvent, decode_bytes, snapshot_from_entries};
use crate::protocol::{ExecutionEvent, OutputStream, ShellEvent};
use crate::{Result, SandboxError};

impl GuestRpcClient {
    pub(super) async fn read_loop(self, reader: tokio::io::ReadHalf<tokio::net::UnixStream>) {
        let mut lines = BufReader::new(reader).lines();
        loop {
            match lines.next_line().await {
                Ok(Some(line)) => {
                    if let Err(error) = self.handle_line(&line).await {
                        self.fail_all(error.to_string()).await;
                        break;
                    }
                }
                Ok(None) => {
                    self.fail_all("guest RPC connection closed".into()).await;
                    break;
                }
                Err(error) => {
                    self.fail_all(format!("guest RPC read error: {error}"))
                        .await;
                    break;
                }
            }
        }
    }

    async fn handle_line(&self, line: &str) -> Result<()> {
        match decode_event(line)? {
            GuestEvent::Ready { ready } => {
                let _ignored = self.inner.ready_tx.send(Some(ready));
            }
            GuestEvent::Pong { request_id } => {
                self.resolve_response(request_id, Ok(GuestResponse::Pong))
                    .await
            }
            GuestEvent::Ack { request_id } => {
                self.resolve_response(request_id, Ok(GuestResponse::Ack))
                    .await
            }
            GuestEvent::ShellOpened {
                request_id,
                session_id,
            } => {
                self.resolve_response(request_id, Ok(GuestResponse::ShellOpened(session_id)))
                    .await
            }
            GuestEvent::ExecOutput {
                exec_id,
                stream,
                data,
            } => self.route_exec_output(exec_id, stream, &data).await?,
            GuestEvent::ExecExit { exec_id, status } => {
                if let Some(sender) = self.inner.exec_streams.lock().await.remove(&exec_id) {
                    let _ignored = sender.send(ExecutionEvent::Exit { status }).await;
                }
            }
            GuestEvent::ShellOutput { session_id, data } => {
                if let Some(sender) = self
                    .inner
                    .shell_streams
                    .lock()
                    .await
                    .get(&session_id)
                    .cloned()
                {
                    let _ignored = sender.send(ShellEvent::Output(decode_bytes(&data)?)).await;
                }
            }
            GuestEvent::ShellExit { session_id, code } => {
                if let Some(sender) = self.inner.shell_streams.lock().await.remove(&session_id) {
                    let _ignored = sender.send(ShellEvent::Exit(code)).await;
                }
            }
            GuestEvent::WorkspaceSnapshot {
                request_id,
                entries,
            } => {
                self.resolve_response(
                    request_id,
                    Ok(GuestResponse::Snapshot(snapshot_from_entries(entries))),
                )
                .await;
            }
            GuestEvent::RuntimeStats { request_id, stats } => {
                self.resolve_response(request_id, Ok(GuestResponse::RuntimeStats(stats)))
                    .await;
            }
            GuestEvent::FilesListed {
                request_id,
                entries,
            } => {
                self.resolve_response(request_id, Ok(GuestResponse::Files(entries)))
                    .await;
            }
            GuestEvent::FileRead { request_id, file } => {
                self.resolve_response(request_id, file.into_read_file().map(GuestResponse::File))
                    .await;
            }
            GuestEvent::Error {
                request_id,
                target,
                message,
            } => self.route_error(request_id, target, &message).await?,
        }
        Ok(())
    }

    async fn route_exec_output(
        &self,
        exec_id: Uuid,
        stream: OutputStream,
        data: &str,
    ) -> Result<()> {
        if let Some(sender) = self.inner.exec_streams.lock().await.get(&exec_id).cloned() {
            let _ignored = sender
                .send(ExecutionEvent::Output {
                    stream,
                    data: decode_bytes(data)?,
                })
                .await;
        }
        Ok(())
    }

    async fn route_error(
        &self,
        request_id: Option<String>,
        target: Option<Uuid>,
        message: &str,
    ) -> Result<()> {
        if let Some(id) = request_id {
            self.resolve_response(id, Err(SandboxError::protocol(message.to_string())))
                .await;
            return Ok(());
        }
        if let Some(exec_id) = target {
            if let Some(sender) = self.inner.exec_streams.lock().await.remove(&exec_id) {
                let _ignored = sender
                    .send(ExecutionEvent::Output {
                        stream: OutputStream::Stderr,
                        data: message.as_bytes().to_vec(),
                    })
                    .await;
                let _ignored = sender
                    .send(ExecutionEvent::Exit {
                        status: crate::protocol::ExecExit::Killed,
                    })
                    .await;
                return Ok(());
            }
            if let Some(sender) = self.inner.shell_streams.lock().await.remove(&exec_id) {
                let _ignored = sender
                    .send(ShellEvent::Output(message.as_bytes().to_vec()))
                    .await;
                let _ignored = sender.send(ShellEvent::Exit(-1)).await;
                return Ok(());
            }
        }
        Err(SandboxError::protocol(message.to_string()))
    }

    async fn resolve_response(&self, request_id: String, response: Result<GuestResponse>) {
        if let Some(sender) = self.inner.responses.lock().await.remove(&request_id) {
            let _ignored = sender.send(response);
        }
    }

    async fn fail_all(&self, message: String) {
        let responses = std::mem::take(&mut *self.inner.responses.lock().await);
        let execs = std::mem::take(&mut *self.inner.exec_streams.lock().await);
        let shells = std::mem::take(&mut *self.inner.shell_streams.lock().await);
        for sender in responses.into_values() {
            let _ignored = sender.send(Err(SandboxError::protocol(message.clone())));
        }
        for sender in execs.into_values() {
            let _ignored = sender
                .send(ExecutionEvent::Output {
                    stream: OutputStream::Stderr,
                    data: message.clone().into_bytes(),
                })
                .await;
            let _ignored = sender
                .send(ExecutionEvent::Exit {
                    status: crate::protocol::ExecExit::Killed,
                })
                .await;
        }
        for sender in shells.into_values() {
            let _ignored = sender
                .send(ShellEvent::Output(message.clone().into_bytes()))
                .await;
            let _ignored = sender.send(ShellEvent::Exit(-1)).await;
        }
    }
}

fn decode_event(line: &str) -> Result<GuestEvent> {
    serde_json::from_str(line).map_err(|error| SandboxError::json("decoding guest event", error))
}
