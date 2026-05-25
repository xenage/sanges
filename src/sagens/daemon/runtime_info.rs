use std::path::{Path, PathBuf};

use crate::config::IsolationMode;
use crate::private_fs::{ensure_private_dir, write_private_file};
use crate::{Result, SandboxError};

const DAEMON_RUNTIME_INFO_FILE: &str = "daemon.runtime.json";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct DaemonRuntimeInfo {
    pub isolation_mode: IsolationMode,
}

pub(crate) async fn read_runtime_info(state_dir: &Path) -> Result<Option<DaemonRuntimeInfo>> {
    match tokio::fs::read(runtime_info_path(state_dir)).await {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|error| SandboxError::json("decoding daemon runtime info", error)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(SandboxError::io("reading daemon runtime info", error)),
    }
}

pub(crate) fn write_runtime_info(state_dir: &Path, isolation_mode: IsolationMode) -> Result<()> {
    ensure_private_dir(state_dir, "creating daemon runtime info directory")?;
    let bytes = serde_json::to_vec_pretty(&DaemonRuntimeInfo { isolation_mode })
        .map_err(|error| SandboxError::json("encoding daemon runtime info", error))?;
    write_private_file(
        &runtime_info_path(state_dir),
        &bytes,
        "creating daemon runtime info directory",
        "writing daemon runtime info",
    )
}

fn runtime_info_path(state_dir: &Path) -> PathBuf {
    state_dir.join(DAEMON_RUNTIME_INFO_FILE)
}
