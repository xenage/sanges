use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{RecvTimeoutError, SyncSender, sync_channel};
use std::thread::JoinHandle;
use std::time::Duration;

use tokio::sync::oneshot;

use crate::auth::{AdminCredential, AdminStore, BoxCredentialStore, UserConfig, write_user_config};
use crate::boxes::{BoxManager, LocalBoxService};
use crate::runtime::{AgentSandboxService, SandboxService};
use crate::sagens::config::{build_runtime_config_for_endpoint, validate_host_process_binary};
use crate::sagens::daemon::bootstrap_admin;
use crate::{Result, SandboxError, serve_box_api_websocket};

#[derive(Debug, Clone)]
pub struct EmbeddedDaemonConfig {
    pub state_dir: PathBuf,
    pub user_config_path: PathBuf,
    pub endpoint: String,
    pub admin_credential: AdminCredential,
}

#[derive(Debug, Clone)]
pub struct EmbeddedDaemonInfo {
    pub state_dir: PathBuf,
    pub user_config_path: PathBuf,
    pub user_config: UserConfig,
}

pub struct EmbeddedDaemonHandle {
    info: EmbeddedDaemonInfo,
    stop_tx: Option<oneshot::Sender<()>>,
    thread: Option<JoinHandle<Result<()>>>,
}

impl EmbeddedDaemonHandle {
    pub fn start(config: EmbeddedDaemonConfig) -> Result<Self> {
        let (ready_tx, ready_rx) = sync_channel(1);
        let (stop_tx, stop_rx) = oneshot::channel();
        let thread = std::thread::Builder::new()
            .name("sagens-embedded-daemon".into())
            .spawn(move || run_embedded_daemon_thread(config, ready_tx, stop_rx))
            .map_err(|error| SandboxError::io("spawning embedded daemon thread", error))?;
        let info = match ready_rx.recv_timeout(Duration::from_secs(10)) {
            Ok(Ok(info)) => info,
            Ok(Err(error)) => {
                let _ = thread.join();
                return Err(error);
            }
            Err(RecvTimeoutError::Timeout) => {
                return Err(SandboxError::timeout(
                    "timed out waiting for embedded daemon startup",
                ));
            }
            Err(RecvTimeoutError::Disconnected) => match thread.join() {
                Ok(Ok(())) => {
                    return Err(SandboxError::backend(
                        "embedded daemon exited before reporting startup state",
                    ));
                }
                Ok(Err(error)) => return Err(error),
                Err(_) => {
                    return Err(SandboxError::backend("embedded daemon thread panicked"));
                }
            },
        };
        Ok(Self {
            info,
            stop_tx: Some(stop_tx),
            thread: Some(thread),
        })
    }

    pub fn info(&self) -> &EmbeddedDaemonInfo {
        &self.info
    }

    pub fn close(&mut self) -> Result<()> {
        if let Some(stop_tx) = self.stop_tx.take() {
            let _ = stop_tx.send(());
        }
        let Some(thread) = self.thread.take() else {
            return Ok(());
        };
        match thread.join() {
            Ok(result) => result,
            Err(_) => Err(SandboxError::backend("embedded daemon thread panicked")),
        }
    }
}

impl Drop for EmbeddedDaemonHandle {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

fn run_embedded_daemon_thread(
    config: EmbeddedDaemonConfig,
    ready_tx: SyncSender<Result<EmbeddedDaemonInfo>>,
    stop_rx: oneshot::Receiver<()>,
) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| SandboxError::io("building embedded daemon runtime", error))?;
    runtime.block_on(async move { run_embedded_daemon(config, ready_tx, stop_rx).await })
}

async fn run_embedded_daemon(
    config: EmbeddedDaemonConfig,
    ready_tx: SyncSender<Result<EmbeddedDaemonInfo>>,
    stop_rx: oneshot::Receiver<()>,
) -> Result<()> {
    let host_binary = std::env::current_exe()
        .map_err(|error| SandboxError::io("discovering host process executable", error))?;
    validate_host_process_binary(&host_binary)?;
    let runtime_config = build_runtime_config_for_endpoint(&config.state_dir, &config.endpoint)?;
    let runtime: Arc<dyn SandboxService> =
        Arc::new(AgentSandboxService::new(runtime_config.clone()).await?);
    let service: Arc<dyn BoxManager> = Arc::new(
        LocalBoxService::new(
            runtime_config.state_dir.clone(),
            runtime_config.workspace.clone(),
            runtime_config.default_policy,
            runtime_config.isolation_mode,
            runtime,
        )
        .await?,
    );
    let admin_store = Arc::new(AdminStore::new(&runtime_config.state_dir));
    let box_credential_store = Arc::new(BoxCredentialStore::new(&runtime_config.state_dir));
    bootstrap_admin(&admin_store, &config.admin_credential).await?;
    let server = serve_box_api_websocket(
        runtime_config.control.bind_addr,
        service,
        admin_store,
        box_credential_store,
        runtime_config.isolation_mode,
    )
    .await?;
    let user_config = UserConfig {
        version: 1,
        admin_uuid: config.admin_credential.admin_uuid,
        admin_token: config.admin_credential.admin_token.clone(),
        endpoint: format!("ws://{}", server.addr),
    };
    if let Err(error) = write_user_config(&config.user_config_path, &user_config).await {
        server.shutdown();
        let _ = server.wait().await;
        return Err(error);
    }
    let _ = ready_tx.send(Ok(EmbeddedDaemonInfo {
        state_dir: config.state_dir,
        user_config_path: config.user_config_path,
        user_config,
    }));
    let shutdown = server.shutdown_signal();
    let wait = server.wait();
    tokio::pin!(wait);
    tokio::select! {
        result = &mut wait => result,
        _ = stop_rx => {
            let _ = shutdown.send(true);
            wait.await
        }
    }
}
