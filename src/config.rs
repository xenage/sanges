use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::time::Duration;

use crate::{Result, SandboxError};

#[cfg(test)]
#[path = "config/tests.rs"]
mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IsolationMode {
    Compat,
    Secure,
}

impl IsolationMode {
    pub fn default_for_host() -> Self {
        if cfg!(target_os = "linux")
            && std::env::var_os("SAGENS_CGROUP_PARENT").is_some_and(|value| !value.is_empty())
        {
            Self::Secure
        } else {
            Self::Compat
        }
    }
}

#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub state_dir: PathBuf,
    pub guest: GuestConfig,
    pub workspace: WorkspaceConfig,
    pub control: ControlPlaneConfig,
    pub lifecycle: LifecycleConfig,
    pub isolation_mode: IsolationMode,
    pub hardening: HardeningConfig,
    pub artifact_bundle: ArtifactBundle,
    pub default_policy: ExecutionPolicy,
}

impl RuntimeConfig {
    pub fn validate(&self) -> Result<()> {
        if self.state_dir.as_os_str().is_empty() {
            return Err(SandboxError::invalid("state_dir must not be empty"));
        }
        self.guest.validate()?;
        self.workspace.validate()?;
        self.control.validate()?;
        self.lifecycle.validate()?;
        self.hardening.validate(self.isolation_mode)?;
        self.default_policy.validate()?;
        match self.isolation_mode {
            IsolationMode::Compat => {}
            IsolationMode::Secure if cfg!(target_os = "linux") => {}
            IsolationMode::Secure => {
                return Err(SandboxError::invalid(
                    "secure isolation mode is only supported on Linux",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct GuestConfig {
    pub libkrun_library: PathBuf,
    pub kernel_image: PathBuf,
    pub kernel_format: GuestKernelFormat,
    pub rootfs_image: PathBuf,
    pub firmware: Option<PathBuf>,
    pub guest_agent_path: PathBuf,
    pub guest_vsock_port: u32,
    pub boot_timeout: Duration,
    pub guest_uid: u32,
    pub guest_gid: u32,
    pub guest_tmpfs_mib: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum GuestKernelFormat {
    Raw,
    Elf,
    PeGz,
    ImageBz2,
    ImageGz,
    ImageZstd,
}

impl GuestKernelFormat {
    pub fn parse(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "raw" => Ok(Self::Raw),
            "elf" => Ok(Self::Elf),
            "pegz" | "pe_gz" | "pe-gz" => Ok(Self::PeGz),
            "imagebz2" | "image_bz2" | "image-bz2" => Ok(Self::ImageBz2),
            "imagegz" | "image_gz" | "image-gz" => Ok(Self::ImageGz),
            "imagezstd" | "image_zstd" | "image-zstd" => Ok(Self::ImageZstd),
            _ => Err(SandboxError::invalid(format!(
                "unsupported guest kernel format: {value}"
            ))),
        }
    }
}

impl GuestConfig {
    pub fn validate(&self) -> Result<()> {
        for (name, path) in [
            ("libkrun_library", &self.libkrun_library),
            ("rootfs_image", &self.rootfs_image),
            ("guest_agent_path", &self.guest_agent_path),
        ] {
            if path.as_os_str().is_empty() {
                return Err(SandboxError::invalid(format!("{name} must not be empty")));
            }
        }
        if cfg!(all(target_os = "macos", target_arch = "x86_64")) && self.firmware.is_none() {
            return Err(SandboxError::invalid(
                "firmware must be configured for libkrun on macOS",
            ));
        }
        if self.guest_vsock_port < 1024 {
            return Err(SandboxError::invalid(
                "guest_vsock_port must be >= 1024 for sandbox RPC",
            ));
        }
        if self.boot_timeout < Duration::from_secs(1) {
            return Err(SandboxError::invalid(
                "boot_timeout must be at least one second",
            ));
        }
        if self.guest_tmpfs_mib < 32 {
            return Err(SandboxError::invalid("guest_tmpfs_mib must be at least 32"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct WorkspaceConfig {
    pub disk_size_mib: u64,
}

impl WorkspaceConfig {
    pub fn validate(&self) -> Result<()> {
        if self.disk_size_mib < 64 {
            return Err(SandboxError::invalid(
                "workspace disk must be at least 64 MiB",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct ControlPlaneConfig {
    pub bind_addr: SocketAddr,
    pub allow_remote_bind: bool,
}

impl Default for ControlPlaneConfig {
    fn default() -> Self {
        Self {
            bind_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 7000),
            allow_remote_bind: false,
        }
    }
}

impl ControlPlaneConfig {
    pub fn validate(&self) -> Result<()> {
        if !self.allow_remote_bind && !self.bind_addr.ip().is_loopback() {
            return Err(SandboxError::invalid(
                "control-plane bind must stay on loopback unless remote bind is explicitly allowed",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct HardeningConfig {
    pub enable_landlock: bool,
    pub cgroup_parent: Option<PathBuf>,
    pub runner_log_limit_bytes: u64,
}

impl HardeningConfig {
    pub fn validate(&self, isolation_mode: IsolationMode) -> Result<()> {
        if self.runner_log_limit_bytes < 1_048_576 {
            return Err(SandboxError::invalid(
                "runner_log_limit_bytes must be at least 1048576",
            ));
        }
        if isolation_mode == IsolationMode::Secure && self.cgroup_parent.is_none() {
            return Err(SandboxError::invalid(
                "secure isolation mode requires SAGENS_CGROUP_PARENT",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct LifecycleConfig {
    pub idle_timeout: Duration,
    pub shutdown_grace: Duration,
    pub warm_pool_size: usize,
    pub reap_interval: Duration,
}

impl Default for LifecycleConfig {
    fn default() -> Self {
        Self {
            idle_timeout: Duration::from_secs(20),
            shutdown_grace: Duration::from_secs(2),
            warm_pool_size: 1,
            reap_interval: Duration::from_millis(500),
        }
    }
}

impl LifecycleConfig {
    pub fn validate(&self) -> Result<()> {
        if self.idle_timeout < Duration::from_secs(1) {
            return Err(SandboxError::invalid(
                "idle_timeout must be at least one second",
            ));
        }
        if self.shutdown_grace < Duration::from_millis(100) {
            return Err(SandboxError::invalid(
                "shutdown_grace must be at least 100ms",
            ));
        }
        if self.reap_interval < Duration::from_millis(100) {
            return Err(SandboxError::invalid(
                "reap_interval must be at least 100ms",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct ArtifactBundle {
    pub bundle_id: String,
}

impl Default for ArtifactBundle {
    fn default() -> Self {
        Self {
            bundle_id: "dev".into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExecutionPolicy {
    pub cpu_cores: u32,
    pub memory_mb: u32,
    pub max_processes: u32,
    pub network_enabled: bool,
    pub timeout_ms: Option<u64>,
}

impl ExecutionPolicy {
    pub fn validate(&self) -> Result<()> {
        if self.cpu_cores == 0 {
            return Err(SandboxError::invalid("cpu_cores must be at least 1"));
        }
        if self.memory_mb < 128 {
            return Err(SandboxError::invalid("memory_mb must be at least 128"));
        }
        if self.max_processes == 0 {
            return Err(SandboxError::invalid("max_processes must be at least 1"));
        }
        if matches!(self.timeout_ms, Some(0)) {
            return Err(SandboxError::invalid(
                "timeout_ms must be greater than zero when configured",
            ));
        }
        Ok(())
    }
}

impl Default for ExecutionPolicy {
    fn default() -> Self {
        Self {
            cpu_cores: 1,
            memory_mb: 512,
            max_processes: 256,
            network_enabled: false,
            timeout_ms: None,
        }
    }
}

pub type SandboxPolicy = ExecutionPolicy;

#[derive(Debug, Clone)]
pub struct SandboxSpec {
    pub workspace_id: String,
    pub policy: ExecutionPolicy,
    pub restore_commit: Option<String>,
}

impl SandboxSpec {
    pub fn new(workspace_id: impl Into<String>) -> Self {
        Self {
            workspace_id: workspace_id.into(),
            policy: ExecutionPolicy::default(),
            restore_commit: None,
        }
    }
}
