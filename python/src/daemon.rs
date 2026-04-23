use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use pyo3::prelude::*;
use sagens_host::auth::{UserConfig, write_user_config};

use crate::error::runtime_error;
use crate::runtime::block_on;

const DAEMON_WAIT_TIMEOUT: Duration = Duration::from_secs(30);

#[pyclass]
pub struct DaemonProcessHandle {
    user_config_json: String,
    user_config: UserConfig,
    child: Option<Child>,
}

#[pymethods]
impl DaemonProcessHandle {
    #[getter]
    fn user_config_json(&self) -> String {
        self.user_config_json.clone()
    }

    fn close(&mut self) -> PyResult<bool> {
        close_child(self)
    }
}

impl Drop for DaemonProcessHandle {
    fn drop(&mut self) {
        let _ = close_child(self);
    }
}

#[pyfunction]
pub fn spawn_daemon_process(
    host_binary: String,
    state_dir: Option<String>,
    user_config_path: Option<String>,
    endpoint: Option<String>,
) -> PyResult<DaemonProcessHandle> {
    let state_dir = state_dir.map(PathBuf::from).unwrap_or_else(default_state_dir);
    let user_config_path = user_config_path
        .map(PathBuf::from)
        .unwrap_or_else(|| state_dir.join("config.json"));
    let user_config = UserConfig::new(endpoint.unwrap_or_else(default_endpoint));
    block_on(write_user_config(&user_config_path, &user_config))?.map_err(runtime_error)?;
    let child = spawn_process(
        Path::new(&host_binary),
        &state_dir,
        &user_config_path,
        &user_config,
    )
    .map_err(runtime_error)?;
    wait_for_daemon(&user_config)?;
    let user_config_json = serde_json::to_string(&user_config).map_err(runtime_error)?;
    Ok(DaemonProcessHandle {
        user_config_json,
        user_config,
        child: Some(child),
    })
}

#[pyfunction]
pub fn quit_daemon(
    state_dir: Option<String>,
    user_config_path: Option<String>,
    endpoint: Option<String>,
) -> PyResult<bool> {
    let state_dir = state_dir.map(PathBuf::from).unwrap_or_else(default_state_dir);
    let config_path = user_config_path
        .map(PathBuf::from)
        .unwrap_or_else(|| state_dir.join("config.json"));
    let mut user_config = block_on(sagens_host::auth::read_user_config(&config_path))?
        .map_err(runtime_error)?;
    if let Some(endpoint) = endpoint {
        user_config.endpoint = endpoint;
    }
    block_on(async { try_quit_daemon(&user_config).await })?.map_err(runtime_error)
}

#[pyfunction]
pub fn read_user_config_json(path: String) -> PyResult<String> {
    let config = block_on(sagens_host::auth::read_user_config(Path::new(&path)))?
        .map_err(runtime_error)?;
    serde_json::to_string(&config).map_err(runtime_error)
}

fn close_child(handle: &mut DaemonProcessHandle) -> PyResult<bool> {
    let Some(mut child) = handle.child.take() else {
        return Ok(false);
    };
    block_on(async { shutdown_if_running(&handle.user_config).await })?.map_err(runtime_error)?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if child
            .try_wait()
            .map_err(runtime_error)?
            .is_some()
        {
            return Ok(true);
        }
        if Instant::now() >= deadline {
            child
                .kill()
                .map_err(runtime_error)?;
            let _ = child.wait();
            return Ok(true);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn spawn_process(
    host_binary: &Path,
    state_dir: &Path,
    user_config_path: &Path,
    user_config: &UserConfig,
) -> sagens_host::Result<Child> {
    std::fs::create_dir_all(state_dir).map_err(|error| {
        sagens_host::SandboxError::io("creating python daemon state dir", error)
    })?;
    let log_path = state_dir.join("daemon.log");
    let stdout = std::fs::File::create(&log_path)
        .map_err(|error| sagens_host::SandboxError::io("creating python daemon log", error))?;
    let stderr = stdout
        .try_clone()
        .map_err(|error| sagens_host::SandboxError::io("cloning python daemon log", error))?;
    Command::new(host_binary)
        .arg("daemon")
        .env("SAGENS_STATE_DIR", state_dir)
        .env("SAGENS_CONFIG", user_config_path)
        .env("SAGENS_ENDPOINT", &user_config.endpoint)
        .env("SAGENS_BOOTSTRAP_ADMIN_UUID", user_config.admin_uuid.to_string())
        .env("SAGENS_BOOTSTRAP_ADMIN_TOKEN", &user_config.admin_token)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .map_err(|error| sagens_host::SandboxError::io("spawning python daemon child", error))
}

fn wait_for_daemon(user_config: &UserConfig) -> PyResult<()> {
    let deadline = Instant::now() + DAEMON_WAIT_TIMEOUT;
    loop {
        if block_on(async { daemon_is_healthy(user_config).await })?.map_err(runtime_error)? {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(runtime_error(format!(
                "timed out waiting for daemon at {}",
                user_config.endpoint
            )));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

async fn daemon_is_healthy(user_config: &UserConfig) -> sagens_host::Result<bool> {
    let client = match sagens_host::BoxApiClient::connect(user_config).await {
        Ok(client) => client,
        Err(_) => return Ok(false),
    };
    Ok(client.list_boxes().await.is_ok())
}

async fn shutdown_if_running(user_config: &UserConfig) -> sagens_host::Result<()> {
    if let Ok(client) = sagens_host::BoxApiClient::connect(user_config).await {
        let _ = client.shutdown_daemon().await;
    }
    Ok(())
}

async fn try_quit_daemon(user_config: &UserConfig) -> sagens_host::Result<bool> {
    let client = match sagens_host::BoxApiClient::connect(user_config).await {
        Ok(client) => client,
        Err(_) => return Ok(false),
    };
    let _ = client.shutdown_daemon().await;
    Ok(true)
}

fn default_state_dir() -> PathBuf {
    std::env::temp_dir().join("sagens-python")
}

fn default_endpoint() -> String {
    "ws://127.0.0.1:7000".into()
}
