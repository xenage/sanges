use std::path::{Path, PathBuf};

use tokio::fs;

use crate::backend::BackendLaunchRequest;
use crate::config::{GuestKernelFormat, IsolationMode};
use crate::{Result, SandboxError};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LibkrunRunnerConfig {
    pub kernel_image: PathBuf,
    pub kernel_format: GuestKernelFormat,
    pub rootfs_image: PathBuf,
    pub workspace_image: PathBuf,
    pub runtime_dir: PathBuf,
    pub console_output_path: PathBuf,
    pub firmware: Option<PathBuf>,
    pub guest_agent_path: PathBuf,
    pub cpu_cores: u32,
    pub memory_mb: u32,
    pub tmpfs_mib: u32,
    pub max_processes: u32,
    pub network_enabled: bool,
    pub guest_uid: u32,
    pub guest_gid: u32,
    pub guest_vsock_port: u32,
    pub vsock_socket: PathBuf,
    pub isolation_mode: IsolationMode,
    pub runner_log_limit_bytes: u64,
}

impl LibkrunRunnerConfig {
    pub fn kernel_cmdline(&self) -> String {
        let max_open_files = self.max_processes.saturating_mul(16).clamp(256, 4096);
        if cfg!(target_os = "linux") && self.uses_krun_init() {
            return format!(
                "reboot=k panic=-1 panic_print=0 nomodule console=hvc0 root=/dev/root rootfstype=virtiofs rw quiet no-kvmapf init=/init.krun sandbox.workspace_device=/dev/vdb sandbox.tmpfs_mib={} sandbox.uid={} sandbox.gid={} sandbox.max_processes={} sandbox.max_open_files={} sandbox.max_file_size_bytes={} sandbox.rpc_port={} sandbox.network_enabled={}",
                self.tmpfs_mib,
                self.guest_uid,
                self.guest_gid,
                self.max_processes,
                max_open_files,
                16 * 1024 * 1024u64,
                self.guest_vsock_port,
                if self.network_enabled { 1 } else { 0 },
            );
        }
        #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
        let rpc_transport = " sandbox.rpc_transport=virtio-serial";
        #[cfg(not(all(target_os = "macos", target_arch = "x86_64")))]
        let rpc_transport = "";
        format!(
            "console=hvc0 root={} ro rootfstype=ext4 rootwait loglevel=8 ignore_loglevel sandbox.workspace_device=/dev/vdb sandbox.tmpfs_mib={} sandbox.uid={} sandbox.gid={} sandbox.max_processes={} sandbox.max_open_files={} sandbox.max_file_size_bytes={} sandbox.rpc_port={} sandbox.network_enabled={}{} panic=-1",
            self.root_device(),
            self.tmpfs_mib,
            self.guest_uid,
            self.guest_gid,
            self.max_processes,
            max_open_files,
            16 * 1024 * 1024u64,
            self.guest_vsock_port,
            if self.network_enabled { 1 } else { 0 },
            rpc_transport,
        )
    }

    pub fn uses_krun_init(&self) -> bool {
        cfg!(target_os = "linux")
            && self.firmware.is_none()
            && self.kernel_format == GuestKernelFormat::Raw
    }

    pub fn root_device(&self) -> &'static str {
        "/dev/vda"
    }
}

pub fn build_runner_config(request: &BackendLaunchRequest) -> LibkrunRunnerConfig {
    LibkrunRunnerConfig {
        kernel_image: request.guest.kernel_image.clone(),
        kernel_format: request.guest.kernel_format,
        rootfs_image: request.guest.rootfs_image.clone(),
        workspace_image: request.workspace.disk_path.clone(),
        runtime_dir: request.run_layout.runtime_dir.clone(),
        console_output_path: request.run_layout.guest_console_log.clone(),
        firmware: request.guest.firmware.clone(),
        guest_agent_path: request.guest.guest_agent_path.clone(),
        cpu_cores: request.policy.cpu_cores,
        memory_mb: request.policy.memory_mb,
        tmpfs_mib: request.guest.guest_tmpfs_mib,
        max_processes: request.policy.max_processes,
        network_enabled: request.policy.network_enabled,
        guest_uid: request.guest.guest_uid,
        guest_gid: request.guest.guest_gid,
        guest_vsock_port: request.guest.guest_vsock_port,
        vsock_socket: request.run_layout.vsock_socket.clone(),
        isolation_mode: request.isolation_mode,
        runner_log_limit_bytes: request.hardening.runner_log_limit_bytes,
    }
}

pub async fn write_debug_runner_config(path: &Path, value: &LibkrunRunnerConfig) -> Result<()> {
    write_json(path, value).await
}

pub fn read_runner_config(path: &Path) -> Result<LibkrunRunnerConfig> {
    let bytes = std::fs::read(path)
        .map_err(|error| SandboxError::io("reading libkrun runner config", error))?;
    serde_json::from_slice(&bytes)
        .map_err(|error| SandboxError::json("decoding libkrun runner config", error))
}

async fn write_json(path: &Path, value: &LibkrunRunnerConfig) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|error| SandboxError::json("encoding libkrun runner config", error))?;
    fs::write(path, bytes)
        .await
        .map_err(|error| SandboxError::io("writing libkrun runner config", error))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::LibkrunRunnerConfig;
    use crate::config::{GuestKernelFormat, IsolationMode};

    fn runner_config() -> LibkrunRunnerConfig {
        LibkrunRunnerConfig {
            kernel_image: PathBuf::from("/tmp/vmlinuz-virt"),
            kernel_format: GuestKernelFormat::Raw,
            rootfs_image: PathBuf::from("/tmp/rootfs.raw"),
            workspace_image: PathBuf::from("/tmp/workspace.raw"),
            runtime_dir: PathBuf::from("/tmp/runtime"),
            console_output_path: PathBuf::from("/tmp/guest-console.log"),
            firmware: None,
            guest_agent_path: PathBuf::from("/usr/local/bin/sagens-guest-agent"),
            cpu_cores: 1,
            memory_mb: 128,
            tmpfs_mib: 64,
            max_processes: 256,
            network_enabled: false,
            guest_uid: 65_534,
            guest_gid: 65_534,
            guest_vsock_port: 11_000,
            vsock_socket: PathBuf::from("/tmp/guest.sock"),
            isolation_mode: IsolationMode::Compat,
            runner_log_limit_bytes: 4 * 1024 * 1024,
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_uses_krun_init_cmdline_without_firmware() {
        let config = runner_config();
        let cmdline = config.kernel_cmdline();
        assert!(config.uses_krun_init());
        assert!(cmdline.contains("init=/init.krun"));
        assert!(cmdline.contains("root=/dev/root"));
        assert!(cmdline.contains("rootfstype=virtiofs"));
        assert!(cmdline.contains("sandbox.workspace_device=/dev/vdb"));
        assert!(cmdline.contains("sandbox.rpc_port=11000"));
    }

    #[test]
    fn firmware_forces_direct_root_boot_cmdline() {
        let mut config = runner_config();
        config.firmware = Some(PathBuf::from("/tmp/fw.fd"));
        let cmdline = config.kernel_cmdline();
        assert!(!config.uses_krun_init());
        assert!(cmdline.contains(&format!("root={}", config.root_device())));
        assert!(!cmdline.contains("init=/init.krun"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn non_raw_linux_kernel_uses_direct_root_boot_cmdline() {
        let mut config = runner_config();
        config.kernel_format = GuestKernelFormat::ImageGz;
        let cmdline = config.kernel_cmdline();
        assert!(!config.uses_krun_init());
        assert!(cmdline.contains(&format!("root={}", config.root_device())));
        assert!(!cmdline.contains("init=/init.krun"));
    }
}
