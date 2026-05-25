use std::sync::mpsc::{RecvTimeoutError, SyncSender, sync_channel};
use std::thread::JoinHandle;
use std::time::Duration;

use pyo3::prelude::*;
use sagens_host::auth::{AdminCredential, AdminStore, BoxCredentialStore, UserConfig};
use sagens_host::boxes::BoxManager;
use sagens_host::serve_box_api_websocket;
use sagens_host::{GuestConfig, GuestKernelFormat, ImageApiConfig};
use tokio::sync::oneshot;
use uuid::Uuid;

use crate::error::runtime_error;
use crate::stub::StubBoxManager;

const READY_TIMEOUT: Duration = Duration::from_secs(10);

#[pyclass]
pub struct TestServerHandle {
    user_config_json: String,
    stop_tx: Option<oneshot::Sender<()>>,
    thread: Option<JoinHandle<sagens_host::Result<()>>>,
}

#[pymethods]
impl TestServerHandle {
    #[getter]
    fn user_config_json(&self) -> String {
        self.user_config_json.clone()
    }

    fn close(&mut self) -> PyResult<()> {
        close_handle(self).map_err(runtime_error)
    }
}

impl Drop for TestServerHandle {
    fn drop(&mut self) {
        let _ = close_handle(self);
    }
}

#[pyfunction]
pub fn start_test_server() -> PyResult<TestServerHandle> {
    let (ready_tx, ready_rx) = sync_channel(1);
    let (stop_tx, stop_rx) = oneshot::channel();
    let thread = std::thread::Builder::new()
        .name("sagens-python-test-server".into())
        .spawn(move || run_server_thread(ready_tx, stop_rx))
        .map_err(runtime_error)?;
    let user_config_json = match ready_rx.recv_timeout(READY_TIMEOUT) {
        Ok(result) => result.map_err(runtime_error)?,
        Err(RecvTimeoutError::Timeout) => {
            return Err(runtime_error("timed out waiting for python test server"));
        }
        Err(RecvTimeoutError::Disconnected) => {
            return Err(runtime_error("python test server exited"));
        }
    };
    Ok(TestServerHandle {
        user_config_json,
        stop_tx: Some(stop_tx),
        thread: Some(thread),
    })
}

fn close_handle(handle: &mut TestServerHandle) -> sagens_host::Result<()> {
    if let Some(stop_tx) = handle.stop_tx.take() {
        let _ = stop_tx.send(());
    }
    let Some(thread) = handle.thread.take() else {
        return Ok(());
    };
    match thread.join() {
        Ok(result) => result,
        Err(_) => Err(sagens_host::SandboxError::backend(
            "python test server thread panicked",
        )),
    }
}

fn run_server_thread(
    ready_tx: SyncSender<sagens_host::Result<String>>,
    stop_rx: oneshot::Receiver<()>,
) -> sagens_host::Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| {
            sagens_host::SandboxError::io("building python test server runtime", error)
        })?;
    runtime.block_on(async move { run_server(ready_tx, stop_rx).await })
}

async fn run_server(
    ready_tx: SyncSender<sagens_host::Result<String>>,
    stop_rx: oneshot::Receiver<()>,
) -> sagens_host::Result<()> {
    let service: std::sync::Arc<dyn BoxManager> = std::sync::Arc::new(StubBoxManager::default());
    let state_dir = tempfile::tempdir()
        .map_err(|error| sagens_host::SandboxError::io("creating python test state dir", error))?
        .keep();
    let admin_store = std::sync::Arc::new(AdminStore::new(&state_dir));
    let box_credential_store = std::sync::Arc::new(BoxCredentialStore::new(&state_dir));
    let admin = AdminCredential {
        admin_uuid: Uuid::new_v4(),
        admin_token: "python-test-admin-token".into(),
    };
    admin_store.bootstrap(&admin).await?;
    let server = serve_box_api_websocket(
        "127.0.0.1:0".parse().expect("loopback addr"),
        service,
        admin_store,
        box_credential_store,
        test_image_api_config(&state_dir),
    )
    .await?;
    let config = UserConfig {
        version: 1,
        admin_uuid: admin.admin_uuid,
        admin_token: admin.admin_token,
        endpoint: format!("ws://{}", server.addr),
    };
    let payload = serde_json::to_string(&config).map_err(runtime_error_string)?;
    let _ = ready_tx.send(Ok(payload));
    let _ = stop_rx.await;
    server.shutdown();
    server.wait().await
}

fn runtime_error_string(error: impl std::fmt::Display) -> sagens_host::SandboxError {
    sagens_host::SandboxError::backend(error.to_string())
}

fn test_image_api_config(state_dir: &std::path::Path) -> ImageApiConfig {
    ImageApiConfig {
        state_dir: state_dir.to_path_buf(),
        base_guest: GuestConfig {
            kernel_image: state_dir.join("vmlinuz"),
            kernel_format: GuestKernelFormat::Raw,
            rootfs_image: state_dir.join("rootfs.raw"),
            firmware: None,
            guest_agent_path: state_dir.join("sagens-guest-agent"),
            guest_vsock_port: 11_000,
            boot_timeout: Duration::from_secs(1),
            guest_uid: 1000,
            guest_gid: 1000,
            guest_tmpfs_mib: 64,
        },
    }
}
