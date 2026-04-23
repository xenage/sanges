use std::path::{Path, PathBuf};

use crate::auth::{UserConfig, read_user_config};
use crate::sagens::config::{SagensPaths, resolve_paths};
use crate::Result;

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ManagedDaemonOptions {
    pub state_dir: Option<PathBuf>,
    pub user_config_path: Option<PathBuf>,
    pub endpoint: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ManagedDaemonPaths {
    pub state_dir: PathBuf,
    pub user_config_path: PathBuf,
    pub endpoint: String,
    pub pid_path: PathBuf,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ManagedDaemonStartInfo {
    pub paths: ManagedDaemonPaths,
    pub user_config: UserConfig,
    pub already_running: bool,
}

pub fn resolve_managed_daemon_paths(options: ManagedDaemonOptions) -> ManagedDaemonPaths {
    let defaults = resolve_paths();
    let state_dir = options.state_dir.unwrap_or(defaults.state_dir);
    ManagedDaemonPaths {
        pid_path: state_dir.join("daemon.pid"),
        state_dir,
        user_config_path: options
            .user_config_path
            .unwrap_or(defaults.user_config_path),
        endpoint: options.endpoint.unwrap_or(defaults.endpoint),
    }
}

pub async fn start_managed_daemon(
    host_binary: &Path,
    options: ManagedDaemonOptions,
) -> Result<ManagedDaemonStartInfo> {
    let paths = resolve_managed_daemon_paths(options);
    let (user_config, already_running) =
        crate::sagens::daemon::ensure_started(&into_internal_paths(&paths), host_binary).await?;
    Ok(ManagedDaemonStartInfo {
        paths,
        user_config,
        already_running,
    })
}

pub async fn quit_managed_daemon(options: ManagedDaemonOptions) -> Result<bool> {
    let paths = resolve_managed_daemon_paths(options);
    crate::sagens::daemon::quit(&into_internal_paths(&paths)).await
}

pub async fn read_managed_user_config(path: &Path) -> Result<UserConfig> {
    read_user_config(path).await
}

fn into_internal_paths(paths: &ManagedDaemonPaths) -> SagensPaths {
    SagensPaths {
        state_dir: paths.state_dir.clone(),
        user_config_path: paths.user_config_path.clone(),
        endpoint: paths.endpoint.clone(),
        pid_path: paths.state_dir.join("daemon.pid"),
    }
}
