use crate::Result;
use crate::box_api::protocol::{BoxEvent, BoxRequest};
use crate::protocol::{CompletedExecution, ExecExit, exit_code as structured_exit_code};

use super::{BoxApiClient, remote_error};

impl BoxApiClient {
    pub(super) async fn collect_exec(&self, request: BoxRequest) -> Result<CompletedExecution> {
        let (request_id, mut events) = self.open_exec_channel(request).await?;
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut output = Vec::new();
        let mut exit_status = ExecExit::Killed;
        while let Some(event) = events.recv().await {
            match event {
                BoxEvent::ExecOutput {
                    request_id: id,
                    stream,
                    data,
                } if id == request_id => {
                    let bytes = super::decode_bytes("exec payload", &data)?;
                    output.extend_from_slice(&bytes);
                    match stream {
                        crate::OutputStream::Stdout => stdout.extend(bytes),
                        crate::OutputStream::Stderr => stderr.extend(bytes),
                    }
                }
                BoxEvent::ExecExit {
                    request_id: id,
                    status,
                } if id == request_id => {
                    exit_status = status;
                    break;
                }
                BoxEvent::Error {
                    request_id: Some(id),
                    message,
                } if id == request_id => return Err(remote_error(message)),
                BoxEvent::Error {
                    request_id: None,
                    message,
                } => return Err(remote_error(message)),
                _ => {}
            }
        }
        Ok(CompletedExecution {
            exit_code: structured_exit_code(&exit_status),
            exit_status,
            output,
            stdout,
            stderr,
        })
    }
}

pub(super) fn exit_code(status: &ExecExit) -> i32 {
    match status {
        ExecExit::Success => structured_exit_code(status).unwrap_or(0),
        ExecExit::ExitCode(code) => structured_exit_code(status).unwrap_or(*code),
        ExecExit::Timeout => 124,
        ExecExit::Killed => 137,
    }
}
