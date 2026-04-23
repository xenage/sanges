//! Internal-first Rust library for the `sagens` host binary.
//!
//! The supported product surface is the `sagens` CLI and the `box_api`
//! WebSocket protocol. The Rust modules below primarily exist to support the
//! shipped binary, integration tests, and local embedding flows.

#[doc(hidden)]
pub mod auth;
#[doc(hidden)]
pub mod backend;
pub mod box_api;
#[doc(hidden)]
pub mod boxes;
#[doc(hidden)]
pub mod bundle;
pub mod config;
pub mod daemon_api;
#[doc(hidden)]
pub mod embedding;
pub mod error;
#[doc(hidden)]
pub mod guest_rpc;
#[doc(hidden)]
pub mod guest_transport;
#[doc(hidden)]
pub mod host_hardening;
#[doc(hidden)]
pub mod protocol;
#[doc(hidden)]
pub mod runtime;
#[doc(hidden)]
pub mod sagens;
#[doc(hidden)]
pub mod workspace;

pub use auth::{
    AdminCredential, AdminCredentialBundle, AdminStore, BoxCredentialBundle, BoxCredentialStore,
    UserConfig,
};
pub use box_api::{
    BoxApiClient, BoxApiServerHandle, BoxEvent, BoxRequest, BoxResponse, BoxShell, ClientMessage,
    InteractiveTarget as BoxApiInteractiveTarget, Principal, ServerMessage,
    serve_box_api_websocket,
};
pub use boxes::{BoxRecord, BoxRuntimeUsage, BoxSettingValue, BoxSettings, BoxStatus};
pub use config::{
    ArtifactBundle, ControlPlaneConfig, ExecutionPolicy, GuestConfig, GuestKernelFormat,
    HardeningConfig, IsolationMode, LifecycleConfig, RuntimeConfig, SandboxPolicy, WorkspaceConfig,
};
pub use daemon_api::{
    ManagedDaemonOptions, ManagedDaemonPaths, ManagedDaemonStartInfo, quit_managed_daemon,
    resolve_managed_daemon_paths, start_managed_daemon,
};
pub use embedding::{EmbeddedDaemonConfig, EmbeddedDaemonHandle, EmbeddedDaemonInfo};
pub use error::{Result, SandboxError};
pub use protocol::{CompletedExecution, ExecExit, OutputStream, exit_code as exec_exit_code};
pub use workspace::{
    CheckpointRestoreMode, FileKind, FileNode, ReadFileResult, WorkspaceChange,
    WorkspaceChangeKind, WorkspaceCheckpointRecord, WorkspaceCheckpointSummary, WorkspaceSnapshot,
};
