pub mod libkrun;

use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::Result;
use crate::config::{ArtifactBundle, GuestConfig, HardeningConfig, IsolationMode, SandboxPolicy};
use crate::guest_transport::GuestTransportEndpoint;
use crate::protocol::ShellEvent;
use crate::workspace::{RunLayout, WorkspaceLease};

#[derive(Debug, Clone)]
pub struct BackendLaunchRequest {
    pub sandbox_id: Uuid,
    pub run_layout: RunLayout,
    pub guest: GuestConfig,
    pub policy: SandboxPolicy,
    pub workspace: WorkspaceLease,
    pub hardening: HardeningConfig,
    pub isolation_mode: IsolationMode,
    pub artifact_bundle: ArtifactBundle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackendCapabilities {
    pub supports_graceful_shutdown: bool,
    pub supports_vsock: bool,
}

pub struct BackendLaunchOutput {
    pub instance: Arc<dyn BackendInstance>,
    pub guest_endpoint: GuestTransportEndpoint,
}

#[async_trait]
pub trait Backend: Send + Sync {
    async fn launch(&self, request: BackendLaunchRequest) -> Result<BackendLaunchOutput>;
    fn name(&self) -> &'static str;
}

#[async_trait]
pub trait BackendInstance: Send + Sync {
    async fn shutdown(&self) -> Result<()>;
    fn capabilities(&self) -> BackendCapabilities;
}

#[async_trait]
pub trait ShellDriver: Send + Sync {
    async fn send_input(&self, session_id: Uuid, data: Vec<u8>) -> Result<()>;
    async fn resize(&self, session_id: Uuid, cols: u16, rows: u16) -> Result<()>;
    async fn close(&self, session_id: Uuid) -> Result<()>;
}

pub struct ShellSession {
    id: Uuid,
    receiver: mpsc::Receiver<ShellEvent>,
    driver: Arc<dyn ShellDriver>,
}

impl ShellSession {
    pub fn new(
        id: Uuid,
        receiver: mpsc::Receiver<ShellEvent>,
        driver: Arc<dyn ShellDriver>,
    ) -> Self {
        Self {
            id,
            receiver,
            driver,
        }
    }

    pub fn into_parts(self) -> (ShellHandle, mpsc::Receiver<ShellEvent>) {
        (
            ShellHandle {
                id: self.id,
                driver: self.driver,
            },
            self.receiver,
        )
    }
}

#[derive(Clone)]
pub struct ShellHandle {
    id: Uuid,
    driver: Arc<dyn ShellDriver>,
}

impl ShellHandle {
    pub fn id(&self) -> Uuid {
        self.id
    }

    pub async fn send_input(&self, data: impl Into<Vec<u8>>) -> Result<()> {
        self.driver.send_input(self.id, data.into()).await
    }

    pub async fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        self.driver.resize(self.id, cols, rows).await
    }

    pub async fn close(&self) -> Result<()> {
        self.driver.close(self.id).await
    }
}
