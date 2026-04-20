pub mod error;
pub mod guest_rpc;
pub mod protocol;
pub mod workspace;

pub use error::{Result, SandboxError};
pub use guest_rpc::{
    GuestEvent, GuestRequest, GuestRpcReady, GuestRuntimeStats, ReadFilePayload, decode_bytes,
    encode_bytes, snapshot_from_entries,
};
pub use protocol::{ExecExit, ExecRequest, OutputStream, ShellRequest};
pub use workspace::{
    FileKind, FileNode, ReadFileResult, WorkspaceChange, WorkspaceChangeKind, WorkspaceSnapshot,
    normalize_workspace_path, resolve_workspace_path, validate_persisted_id,
};
