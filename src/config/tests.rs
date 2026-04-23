use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::time::Duration;

use super::{
    ArtifactBundle, ControlPlaneConfig, GuestConfig, GuestKernelFormat, HardeningConfig,
    IsolationMode, LifecycleConfig, RuntimeConfig, SandboxPolicy, WorkspaceConfig,
};

#[test]
fn rejects_remote_bind_without_opt_in() {
    let config = RuntimeConfig {
        state_dir: PathBuf::from("/tmp/sagens"),
        guest: guest_config(),
        workspace: WorkspaceConfig { disk_size_mib: 512 },
        control: ControlPlaneConfig {
            bind_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 7000),
            allow_remote_bind: false,
        },
        lifecycle: LifecycleConfig::default(),
        isolation_mode: IsolationMode::Compat,
        hardening: HardeningConfig {
            enable_landlock: false,
            cgroup_parent: None,
            runner_log_limit_bytes: 4 * 1024 * 1024,
        },
        artifact_bundle: ArtifactBundle::default(),
        default_policy: SandboxPolicy::default(),
    };
    assert!(config.validate().is_err());
}

#[test]
fn rejects_reserved_guest_vsock_port() {
    let mut config = guest_config();
    config.guest_vsock_port = 1000;
    assert!(config.validate().is_err());
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[test]
fn macos_aarch64_allows_firmwareless_guest_when_kernel_is_embedded() {
    let mut config = guest_config();
    config.kernel_image = PathBuf::new();
    config.firmware = None;
    assert!(config.validate().is_ok());
}

fn guest_config() -> GuestConfig {
    GuestConfig {
        libkrun_library: PathBuf::from("/usr/lib/libkrun.so"),
        kernel_image: PathBuf::from("/box/vmlinux"),
        kernel_format: if cfg!(target_os = "macos") {
            GuestKernelFormat::PeGz
        } else {
            GuestKernelFormat::Raw
        },
        rootfs_image: PathBuf::from("/box/rootfs.raw"),
        firmware: if cfg!(target_os = "macos") {
            Some(PathBuf::from("/usr/share/libkrun/edk2-aarch64-code.fd"))
        } else {
            None
        },
        guest_agent_path: PathBuf::from("/usr/local/bin/sagens-guest-agent"),
        guest_vsock_port: 11_000,
        boot_timeout: Duration::from_secs(5),
        guest_uid: 65_534,
        guest_gid: 65_534,
        guest_tmpfs_mib: 256,
    }
}

#[test]
fn rejects_secure_mode_without_cgroup_parent() {
    let config = RuntimeConfig {
        state_dir: PathBuf::from("/tmp/sagens"),
        guest: guest_config(),
        workspace: WorkspaceConfig { disk_size_mib: 512 },
        control: ControlPlaneConfig::default(),
        lifecycle: LifecycleConfig::default(),
        isolation_mode: IsolationMode::Secure,
        hardening: HardeningConfig {
            enable_landlock: false,
            cgroup_parent: None,
            runner_log_limit_bytes: 4 * 1024 * 1024,
        },
        artifact_bundle: ArtifactBundle::default(),
        default_policy: SandboxPolicy::default(),
    };
    assert!(config.validate().is_err());
}
