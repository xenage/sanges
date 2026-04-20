use base64::{Engine as _, engine::general_purpose::STANDARD};
use uuid::Uuid;

use crate::protocol::{ExecExit, ExecRequest, OutputStream, ShellRequest};
use crate::workspace::{FileNode, ReadFileResult, WorkspaceSnapshot};
use crate::{Result, SandboxError};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct GuestRpcReady {
    pub protocol_version: u32,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct GuestRuntimeStats {
    pub cpu_millicores: u32,
    pub memory_used_mib: u64,
    pub fs_used_mib: u64,
    pub process_count: u32,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GuestRequest {
    Ping {
        request_id: String,
    },
    Exec {
        request_id: String,
        exec_id: Uuid,
        request: ExecRequest,
    },
    OpenShell {
        request_id: String,
        session_id: Uuid,
        request: ShellRequest,
    },
    ShellInput {
        request_id: String,
        session_id: Uuid,
        data: String,
    },
    ResizeShell {
        request_id: String,
        session_id: Uuid,
        cols: u16,
        rows: u16,
    },
    CloseShell {
        request_id: String,
        session_id: Uuid,
    },
    SnapshotWorkspace {
        request_id: String,
    },
    SyncWorkspace {
        request_id: String,
    },
    RuntimeStats {
        request_id: String,
    },
    ListFiles {
        request_id: String,
        path: String,
    },
    ReadFile {
        request_id: String,
        path: String,
        limit: usize,
    },
    WriteFile {
        request_id: String,
        path: String,
        data: String,
        create_parents: bool,
    },
    MakeDir {
        request_id: String,
        path: String,
        recursive: bool,
    },
    RemovePath {
        request_id: String,
        path: String,
        recursive: bool,
    },
    Shutdown {
        request_id: String,
    },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GuestEvent {
    Ready {
        ready: GuestRpcReady,
    },
    Pong {
        request_id: String,
    },
    Ack {
        request_id: String,
    },
    ShellOpened {
        request_id: String,
        session_id: Uuid,
    },
    ExecOutput {
        exec_id: Uuid,
        stream: OutputStream,
        data: String,
    },
    ExecExit {
        exec_id: Uuid,
        status: ExecExit,
    },
    ShellOutput {
        session_id: Uuid,
        data: String,
    },
    ShellExit {
        session_id: Uuid,
        code: i32,
    },
    WorkspaceSnapshot {
        request_id: String,
        entries: Vec<FileNode>,
    },
    RuntimeStats {
        request_id: String,
        stats: GuestRuntimeStats,
    },
    FilesListed {
        request_id: String,
        entries: Vec<FileNode>,
    },
    FileRead {
        request_id: String,
        file: ReadFilePayload,
    },
    Error {
        request_id: Option<String>,
        target: Option<Uuid>,
        message: String,
    },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ReadFilePayload {
    pub path: String,
    pub data: String,
    pub truncated: bool,
}

impl ReadFilePayload {
    pub fn from_read_file(file: &ReadFileResult) -> Self {
        Self {
            path: file.path.clone(),
            data: STANDARD.encode(&file.data),
            truncated: file.truncated,
        }
    }

    pub fn into_read_file(self) -> Result<ReadFileResult> {
        Ok(ReadFileResult {
            path: self.path,
            data: STANDARD.decode(self.data).map_err(|error| {
                SandboxError::protocol(format!("invalid base64 payload: {error}"))
            })?,
            truncated: self.truncated,
        })
    }
}

pub fn encode_bytes(bytes: &[u8]) -> String {
    STANDARD.encode(bytes)
}

pub fn decode_bytes(value: &str) -> Result<Vec<u8>> {
    STANDARD
        .decode(value)
        .map_err(|error| SandboxError::protocol(format!("invalid base64 payload: {error}")))
}

pub fn snapshot_from_entries(entries: Vec<FileNode>) -> WorkspaceSnapshot {
    WorkspaceSnapshot::from_entries(entries)
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::{GuestEvent, GuestRequest, GuestRpcReady, GuestRuntimeStats, ReadFilePayload};
    use crate::ReadFileResult;
    use crate::protocol::{ExecRequest, OutputStream};

    #[test]
    fn read_file_payload_round_trips() {
        let payload = ReadFilePayload::from_read_file(&ReadFileResult {
            path: "tracked.txt".into(),
            data: b"hello".to_vec(),
            truncated: false,
        });
        let restored = payload.into_read_file().expect("round trip");
        assert_eq!(restored.path, "tracked.txt");
        assert_eq!(restored.data, b"hello");
    }

    #[test]
    fn guest_wire_messages_serialize_and_deserialize() {
        let request = GuestRequest::Exec {
            request_id: "req-1".into(),
            exec_id: Uuid::nil(),
            request: ExecRequest::python("print('ok')"),
        };
        let event = GuestEvent::Ready {
            ready: GuestRpcReady {
                protocol_version: 3,
                capabilities: vec!["exec".into(), "shell".into()],
            },
        };
        let output = GuestEvent::ExecOutput {
            exec_id: Uuid::nil(),
            stream: OutputStream::Stdout,
            data: "aGVsbG8=".into(),
        };
        let stats = GuestEvent::RuntimeStats {
            request_id: "req-2".into(),
            stats: GuestRuntimeStats {
                cpu_millicores: 125,
                memory_used_mib: 64,
                fs_used_mib: 32,
                process_count: 7,
            },
        };

        let request_json = serde_json::to_string(&request).expect("serialize request");
        let event_json = serde_json::to_string(&event).expect("serialize event");
        let output_json = serde_json::to_string(&output).expect("serialize output");
        let stats_json = serde_json::to_string(&stats).expect("serialize stats");

        let decoded_request: GuestRequest =
            serde_json::from_str(&request_json).expect("deserialize request");
        let decoded_event: GuestEvent =
            serde_json::from_str(&event_json).expect("deserialize event");
        let decoded_output: GuestEvent =
            serde_json::from_str(&output_json).expect("deserialize output");
        let decoded_stats: GuestEvent =
            serde_json::from_str(&stats_json).expect("deserialize stats");

        assert!(matches!(decoded_request, GuestRequest::Exec { .. }));
        assert!(matches!(decoded_event, GuestEvent::Ready { .. }));
        assert!(matches!(decoded_output, GuestEvent::ExecOutput { .. }));
        assert!(matches!(decoded_stats, GuestEvent::RuntimeStats { .. }));
    }
}
