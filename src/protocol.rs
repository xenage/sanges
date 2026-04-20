use tokio::sync::OwnedSemaphorePermit;
use tokio::sync::mpsc;

pub use sagens_guest_contract::protocol::{ExecExit, ExecRequest, OutputStream, ShellRequest};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionEvent {
    Output { stream: OutputStream, data: Vec<u8> },
    Exit { status: ExecExit },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellEvent {
    Started,
    Output(Vec<u8>),
    Exit(i32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletedExecution {
    pub exit_status: ExecExit,
    pub exit_code: Option<i32>,
    pub output: Vec<u8>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

pub struct CommandStream {
    receiver: mpsc::Receiver<ExecutionEvent>,
    permit: Option<OwnedSemaphorePermit>,
}

impl CommandStream {
    pub fn new(receiver: mpsc::Receiver<ExecutionEvent>) -> Self {
        Self {
            receiver,
            permit: None,
        }
    }

    pub fn with_exec_permit(mut self, permit: OwnedSemaphorePermit) -> Self {
        self.permit = Some(permit);
        self
    }

    pub async fn next(&mut self) -> Option<ExecutionEvent> {
        self.receiver.recv().await
    }

    pub async fn collect(mut self) -> CompletedExecution {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut output = Vec::new();
        let mut exit_status = ExecExit::Killed;

        while let Some(event) = self.next().await {
            match event {
                ExecutionEvent::Output { stream, data } => {
                    output.extend_from_slice(&data);
                    match stream {
                        OutputStream::Stdout => stdout.extend(data),
                        OutputStream::Stderr => stderr.extend(data),
                    }
                }
                ExecutionEvent::Exit { status } => exit_status = status,
            }
        }

        CompletedExecution {
            exit_code: exit_code(&exit_status),
            exit_status,
            output,
            stdout,
            stderr,
        }
    }
}

pub fn exit_code(status: &ExecExit) -> Option<i32> {
    match status {
        ExecExit::Success => Some(0),
        ExecExit::ExitCode(code) => Some(*code),
        ExecExit::Timeout | ExecExit::Killed => None,
    }
}
