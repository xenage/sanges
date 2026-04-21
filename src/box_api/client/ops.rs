#[path = "ops/admin.rs"]
mod admin;
#[path = "ops/files.rs"]
mod files;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::{Duration, Instant};

use super::exec::exit_code;
use super::tty::spawn_tty_stdin_reader;
use super::{BoxApiClient, BoxShell, decode_bytes};
use crate::box_api::client::terminal::TerminalMode;
use crate::box_api::protocol::{BoxEvent, BoxRequest, BoxResponse, InteractiveTarget};
use crate::boxes::BoxStatus;
use crate::protocol::CompletedExecution;
use crate::{BoxRecord, BoxSettingValue, Result, SandboxError};

impl BoxApiClient {
    pub async fn list_boxes(&self) -> Result<Vec<BoxRecord>> {
        let request_id = self.next_request_id();
        self.request_response(
            BoxRequest::ListBoxes { request_id },
            |response| match response {
                BoxResponse::BoxList { boxes } => Some(boxes),
                _ => None,
            },
        )
        .await
    }

    pub async fn get_box(&self, box_id: uuid::Uuid) -> Result<BoxRecord> {
        let request_id = self.next_request_id();
        self.request_box(BoxRequest::GetBox { request_id, box_id })
            .await
    }

    pub async fn create_box(&self) -> Result<BoxRecord> {
        let request_id = self.next_request_id();
        self.request_box(BoxRequest::NewBox { request_id }).await
    }

    pub async fn start_box(&self, box_id: uuid::Uuid) -> Result<BoxRecord> {
        let request_id = self.next_request_id();
        self.request_box(BoxRequest::StartBox { request_id, box_id })
            .await
    }

    pub async fn stop_box(&self, box_id: uuid::Uuid) -> Result<BoxRecord> {
        let request_id = self.next_request_id();
        match self
            .request_box(BoxRequest::StopBox { request_id, box_id })
            .await
        {
            Ok(record) => Ok(record),
            Err(error) => self.recover_stopped_box(box_id, error).await,
        }
    }

    pub async fn remove_box(&self, box_id: uuid::Uuid) -> Result<()> {
        let request_id = self.next_request_id();
        self.request_response(BoxRequest::RemoveBox { request_id, box_id }, |response| {
            match response {
                BoxResponse::BoxRemoved { .. } => Some(()),
                _ => None,
            }
        })
        .await
    }

    pub async fn set_box_setting(
        &self,
        box_id: uuid::Uuid,
        value: BoxSettingValue,
    ) -> Result<BoxRecord> {
        let request_id = self.next_request_id();
        self.request_box(BoxRequest::SetBoxSetting {
            request_id,
            box_id,
            value,
        })
        .await
    }

    pub async fn exec_bash(&self, box_id: uuid::Uuid, command: String) -> Result<i32> {
        let completed = self.exec_bash_capture(box_id, command).await?;
        let mut stdout = tokio::io::stdout();
        let mut stderr = tokio::io::stderr();
        stdout
            .write_all(&completed.stdout)
            .await
            .map_err(|error| SandboxError::io("writing exec stdout", error))?;
        stderr
            .write_all(&completed.stderr)
            .await
            .map_err(|error| SandboxError::io("writing exec stderr", error))?;
        stdout
            .flush()
            .await
            .map_err(|error| SandboxError::io("flushing exec stdout", error))?;
        stderr
            .flush()
            .await
            .map_err(|error| SandboxError::io("flushing exec stderr", error))?;
        Ok(exit_code(&completed.exit_status))
    }

    pub async fn exec_python(&self, box_id: uuid::Uuid, args: Vec<String>) -> Result<i32> {
        let completed = self.exec_python_capture(box_id, args).await?;
        let mut stdout = tokio::io::stdout();
        let mut stderr = tokio::io::stderr();
        stdout
            .write_all(&completed.stdout)
            .await
            .map_err(|error| SandboxError::io("writing exec stdout", error))?;
        stderr
            .write_all(&completed.stderr)
            .await
            .map_err(|error| SandboxError::io("writing exec stderr", error))?;
        stdout
            .flush()
            .await
            .map_err(|error| SandboxError::io("flushing exec stdout", error))?;
        stderr
            .flush()
            .await
            .map_err(|error| SandboxError::io("flushing exec stderr", error))?;
        Ok(exit_code(&completed.exit_status))
    }

    pub async fn exec_bash_capture(
        &self,
        box_id: uuid::Uuid,
        command: String,
    ) -> Result<CompletedExecution> {
        let request_id = self.next_request_id();
        self.collect_exec(BoxRequest::ExecBash {
            request_id,
            box_id,
            command,
            timeout_ms: None,
            kill_grace_ms: None,
        })
        .await
    }

    pub async fn exec_python_capture(
        &self,
        box_id: uuid::Uuid,
        args: Vec<String>,
    ) -> Result<CompletedExecution> {
        let request_id = self.next_request_id();
        self.collect_exec(BoxRequest::ExecPython {
            request_id,
            box_id,
            args,
            timeout_ms: None,
            kill_grace_ms: None,
        })
        .await
    }

    pub async fn exec_bash_with_timeout(
        &self,
        box_id: uuid::Uuid,
        command: String,
        timeout_ms: u64,
        kill_grace_ms: u64,
    ) -> Result<CompletedExecution> {
        let request_id = self.next_request_id();
        self.collect_exec(BoxRequest::ExecBash {
            request_id,
            box_id,
            command,
            timeout_ms: Some(timeout_ms),
            kill_grace_ms: Some(kill_grace_ms),
        })
        .await
    }

    pub async fn interactive_shell(
        &self,
        box_id: uuid::Uuid,
        target: InteractiveTarget,
    ) -> Result<i32> {
        let terminal = TerminalMode::capture()?;
        if !terminal.is_tty() {
            return self.run_piped_interactive_exec(box_id, target).await;
        }
        #[cfg(unix)]
        let _raw_terminal = terminal.enter_raw_mode()?;
        let shell = self.open_shell(box_id, target).await?;
        let (shell, mut events) = shell.into_parts();
        if let Some((cols, rows)) = terminal.size() {
            shell.resize(cols, rows).await?;
        }
        let mut stdout = tokio::io::stdout();
        let mut close_sent = false;
        let mut stdin_events = spawn_tty_stdin_reader();
        loop {
            tokio::select! {
                event = events.recv() => match event.ok_or_else(|| SandboxError::backend("shell event channel closed"))? {
                    BoxEvent::ShellOutput { data, .. } => {
                        let bytes = decode_bytes("shell payload", &data)?;
                        stdout.write_all(&bytes).await
                            .map_err(|error| SandboxError::io("writing shell output", error))?;
                        stdout.flush().await
                            .map_err(|error| SandboxError::io("flushing shell output", error))?;
                    }
                    BoxEvent::ShellExit { code, .. } => return Ok(code),
                    BoxEvent::Error { message, .. } => return Err(SandboxError::backend(message)),
                    _ => {}
                },
                read = stdin_events.recv(), if !close_sent => {
                    let Some(read) = read else {
                        shell.close().await?;
                        close_sent = true;
                        continue;
                    };
                    shell.send_input(read).await?;
                }
            }
        }
    }

    pub async fn open_shell(
        &self,
        box_id: uuid::Uuid,
        target: InteractiveTarget,
    ) -> Result<BoxShell> {
        let request_id = self.next_request_id();
        let shell_id = self
            .request_response(
                BoxRequest::OpenShell {
                    request_id,
                    box_id,
                    target,
                },
                |response| match response {
                    BoxResponse::ShellOpened { shell_id, .. } => Some(shell_id),
                    _ => None,
                },
            )
            .await?;
        let events = self.register_shell_channel(shell_id).await;
        Ok(BoxShell {
            client: self.inner.clone(),
            next_id: self.next_id.clone(),
            shell_id,
            events: tokio::sync::Mutex::new(events),
        })
    }

    async fn run_piped_interactive_exec(
        &self,
        box_id: uuid::Uuid,
        target: InteractiveTarget,
    ) -> Result<i32> {
        let mut stdin = tokio::io::stdin();
        let mut stdout = tokio::io::stdout();
        let mut stderr = tokio::io::stderr();
        let mut input = Vec::new();
        stdin
            .read_to_end(&mut input)
            .await
            .map_err(|error| SandboxError::io("reading piped interactive input", error))?;
        let script = String::from_utf8(input).map_err(|error| {
            SandboxError::invalid(format!("interactive stdin is not valid UTF-8: {error}"))
        })?;
        let delimiter = format!("SAGENS_EOF_{}", uuid::Uuid::new_v4().simple());
        let interpreter = match target {
            InteractiveTarget::Bash => "bash",
            InteractiveTarget::Python => "python3",
        };
        let command = format!("{interpreter} <<'{delimiter}'\n{script}\n{delimiter}\n");
        let completed = self.exec_bash_capture(box_id, command).await?;
        stdout
            .write_all(&completed.stdout)
            .await
            .map_err(|error| SandboxError::io("writing exec stdout", error))?;
        stdout
            .flush()
            .await
            .map_err(|error| SandboxError::io("flushing exec stdout", error))?;
        stderr
            .write_all(&completed.stderr)
            .await
            .map_err(|error| SandboxError::io("writing exec stderr", error))?;
        stderr
            .flush()
            .await
            .map_err(|error| SandboxError::io("flushing exec stderr", error))?;
        Ok(exit_code(&completed.exit_status))
    }
}

impl BoxApiClient {
    async fn recover_stopped_box(
        &self,
        box_id: uuid::Uuid,
        error: SandboxError,
    ) -> Result<BoxRecord> {
        if !is_connection_lost_error(&error) {
            return Err(error);
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match self.reconnect().await {
                Ok(client) => match client.get_box(box_id).await {
                    Ok(record) if record.status != BoxStatus::Running => return Ok(record),
                    Ok(_) => {}
                    Err(fetch_error) if !is_connection_lost_error(&fetch_error) => {
                        return Err(error);
                    }
                    Err(_) => {}
                },
                Err(reconnect_error) if !is_connection_lost_error(&reconnect_error) => {
                    return Err(error);
                }
                Err(_) => {}
            }
            if Instant::now() >= deadline {
                return Err(error);
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}

fn is_connection_lost_error(error: &SandboxError) -> bool {
    let message = error.to_string();
    message.contains("reading websocket message failed")
        || message.contains("websocket connection closed")
        || message.contains("Connection reset without closing handshake")
}

#[cfg(test)]
mod tests {
    use super::is_connection_lost_error;
    use crate::SandboxError;

    #[test]
    fn detects_connection_reset_errors() {
        assert!(is_connection_lost_error(&SandboxError::backend(
            "reading websocket message failed: WebSocket protocol error: Connection reset without closing handshake",
        )));
        assert!(is_connection_lost_error(&SandboxError::backend(
            "backend failure: websocket connection closed",
        )));
        assert!(!is_connection_lost_error(&SandboxError::backend(
            "BOX is not running",
        )));
    }
}
