#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("sagens-guest-agent only supports Linux guests");
    std::process::exit(1);
}

#[cfg(target_os = "linux")]
mod guest_agent;

#[cfg(target_os = "linux")]
pub use sagens_guest_contract::{Result, SandboxError};

#[cfg(target_os = "linux")]
pub mod guest_rpc {
    pub use sagens_guest_contract::guest_rpc::{
        GuestEvent, GuestRequest, GuestRpcReady, GuestRuntimeStats, ReadFilePayload, decode_bytes,
        encode_bytes, snapshot_from_entries,
    };
}

#[cfg(target_os = "linux")]
pub mod protocol {
    pub use sagens_guest_contract::protocol::{ExecExit, ExecRequest, OutputStream, ShellRequest};
}

#[cfg(target_os = "linux")]
pub mod workspace {
    pub use sagens_guest_contract::workspace::{
        FileKind, FileNode, ReadFileResult, WorkspaceChange, WorkspaceChangeKind,
        WorkspaceSnapshot, normalize_workspace_path, resolve_workspace_path, validate_persisted_id,
    };
}

#[cfg(target_os = "linux")]
fn main() {
    guest_agent::entry();
}
