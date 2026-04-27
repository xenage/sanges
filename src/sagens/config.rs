use std::env;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
#[cfg(target_os = "macos")]
use std::process::Command;
use std::time::Duration;

use crate::config::IsolationMode;
use crate::{
    ArtifactBundle, ControlPlaneConfig, GuestConfig, GuestKernelFormat, HardeningConfig,
    LifecycleConfig, Result, RuntimeConfig, SandboxError, SandboxPolicy, WorkspaceConfig,
};

#[derive(Debug, Clone)]
pub struct SagensPaths {
    pub state_dir: PathBuf,
    pub user_config_path: PathBuf,
    pub endpoint: String,
    pub pid_path: PathBuf,
    pub daemon_log_path: PathBuf,
}

pub fn resolve_paths() -> SagensPaths {
    let state_dir = env::var_os("SAGENS_STATE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(default_state_dir);
    let user_config_path = env::var_os("SAGENS_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(default_user_config_path);
    let endpoint = env::var("SAGENS_ENDPOINT").unwrap_or_else(|_| default_endpoint());
    let pid_path = state_dir.join("daemon.pid");
    let daemon_log_path = state_dir.join("daemon.log");
    SagensPaths {
        state_dir,
        user_config_path,
        endpoint,
        pid_path,
        daemon_log_path,
    }
}

pub fn build_runtime_config_for_endpoint(
    state_dir: &Path,
    endpoint: &str,
) -> Result<RuntimeConfig> {
    let prefer_embedded_assets = crate::bundle::has_embedded_assets();
    let kernel_image = default_guest_path(ProjectArtifactKind::Kernel, prefer_embedded_assets);
    let kernel_format =
        GuestKernelFormat::detect_from_path(&kernel_image, GuestKernelFormat::default_for_host());
    Ok(RuntimeConfig {
        state_dir: state_dir.to_path_buf(),
        guest: GuestConfig {
            kernel_image,
            kernel_format,
            rootfs_image: default_guest_path(ProjectArtifactKind::Rootfs, prefer_embedded_assets),
            firmware: default_firmware_path(prefer_embedded_assets),
            guest_agent_path: PathBuf::from("/usr/local/bin/sagens-guest-agent"),
            guest_vsock_port: env::var("SAGENS_GUEST_VSOCK_PORT")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(11_000),
            boot_timeout: Duration::from_secs(30),
            guest_uid: 65_534,
            guest_gid: 65_534,
            guest_tmpfs_mib: 64,
        },
        workspace: WorkspaceConfig {
            disk_size_mib: env::var("SAGENS_WORKSPACE_MIB")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(128),
        },
        control: ControlPlaneConfig {
            bind_addr: parse_endpoint_addr(endpoint)?,
            allow_remote_bind: false,
        },
        lifecycle: LifecycleConfig::default(),
        isolation_mode: parse_isolation_mode()?,
        hardening: HardeningConfig {
            enable_landlock: env_flag("SAGENS_ENABLE_LANDLOCK"),
            cgroup_parent: env::var("SAGENS_CGROUP_PARENT")
                .ok()
                .filter(|value| !value.is_empty())
                .map(PathBuf::from),
            runner_log_limit_bytes: env::var("SAGENS_RUNNER_LOG_LIMIT_BYTES")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(4 * 1024 * 1024),
        },
        artifact_bundle: ArtifactBundle {
            bundle_id: env::var("SAGENS_BUNDLE_ID").unwrap_or_else(|_| "sagens".into()),
        },
        default_policy: SandboxPolicy::default(),
    })
}

#[derive(Clone, Copy)]
enum ProjectArtifactKind {
    Kernel,
    Rootfs,
    Firmware,
}

fn default_guest_path(kind: ProjectArtifactKind, prefer_embedded_assets: bool) -> PathBuf {
    if prefer_embedded_assets {
        return PathBuf::new();
    }
    required_path(&default_project_artifact_candidates(kind))
}

fn default_firmware_path(prefer_embedded_assets: bool) -> Option<PathBuf> {
    if prefer_embedded_assets {
        return None;
    }
    optional_path(&default_project_artifact_candidates(
        ProjectArtifactKind::Firmware,
    ))
}

pub fn parse_endpoint_addr(endpoint: &str) -> Result<SocketAddr> {
    endpoint
        .strip_prefix("ws://")
        .ok_or_else(|| SandboxError::invalid(format!("unsupported endpoint {endpoint}")))?
        .parse()
        .map_err(|error| {
            SandboxError::invalid(format!("invalid websocket endpoint {endpoint}: {error}"))
        })
}

fn default_state_dir() -> PathBuf {
    if cfg!(target_os = "macos") {
        return home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Library")
            .join("Application Support")
            .join("sagens");
    }
    if let Some(xdg) = env::var_os("XDG_STATE_HOME").filter(|value| !value.is_empty()) {
        return PathBuf::from(xdg).join("sagens");
    }
    home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".local")
        .join("state")
        .join("sagens")
}

fn default_user_config_path() -> PathBuf {
    default_config_dir().join("config.json")
}

fn default_config_dir() -> PathBuf {
    if cfg!(target_os = "macos") {
        return home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Library")
            .join("Application Support")
            .join("sagens");
    }
    if let Some(xdg) = env::var_os("XDG_CONFIG_HOME").filter(|value| !value.is_empty()) {
        return PathBuf::from(xdg).join("sagens");
    }
    home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config")
        .join("sagens")
}

fn default_endpoint() -> String {
    "ws://127.0.0.1:7000".into()
}

fn required_path(candidates: &[PathBuf]) -> PathBuf {
    first_existing_path(candidates).unwrap_or_default()
}

fn optional_path(candidates: &[PathBuf]) -> Option<PathBuf> {
    first_existing_path(candidates)
}

fn parse_isolation_mode() -> Result<IsolationMode> {
    match env::var("SAGENS_ISOLATION_MODE") {
        Ok(value) if !value.trim().is_empty() => match value.trim().to_ascii_lowercase().as_str() {
            "compat" => Ok(IsolationMode::Compat),
            "secure" => Ok(IsolationMode::Secure),
            _ => Err(SandboxError::invalid(format!(
                "unsupported SAGENS_ISOLATION_MODE: {value}"
            ))),
        },
        _ => Ok(IsolationMode::default_for_host()),
    }
}

fn default_project_artifact_candidates(kind: ProjectArtifactKind) -> Vec<PathBuf> {
    let root = workspace_root();
    let guest_dir = match (env::consts::OS, env::consts::ARCH) {
        ("macos", "x86_64") | ("linux", "x86_64") => Some(root.join("artifacts/alpine-x86_64")),
        ("macos", "aarch64") | ("linux", "aarch64") => Some(root.join("artifacts/alpine-aarch64")),
        _ => None,
    };
    match kind {
        ProjectArtifactKind::Kernel => guest_dir
            .into_iter()
            .flat_map(|dir| default_kernel_candidates(&dir))
            .collect(),
        ProjectArtifactKind::Rootfs => guest_dir
            .into_iter()
            .map(|dir| dir.join("rootfs.raw"))
            .collect(),
        ProjectArtifactKind::Firmware if env::consts::OS == "macos" => {
            vec![root.join("third_party/upstream/libkrun/edk2/KRUN_EFI.silent.fd")]
        }
        ProjectArtifactKind::Firmware => Vec::new(),
    }
}

fn default_kernel_candidates(guest_dir: &Path) -> Vec<PathBuf> {
    vec![guest_dir.join("vmlinuz-virt")]
}

fn workspace_root() -> &'static Path {
    Path::new(env!("SAGENS_WORKSPACE_ROOT"))
}

fn first_existing_path(candidates: &[PathBuf]) -> Option<PathBuf> {
    candidates.iter().find(|path| path.is_file()).cloned()
}

fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME").map(PathBuf::from)
}

fn env_flag(name: &str) -> bool {
    matches!(
        env::var(name).ok().as_deref(),
        Some("1" | "true" | "yes" | "on")
    )
}

pub fn validate_host_process_binary(host_binary: &Path) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        validate_macos_host_binary(host_binary)?;
    }
    #[cfg(not(target_os = "macos"))]
    let _ = host_binary;
    Ok(())
}

#[cfg(target_os = "macos")]
fn validate_macos_host_binary(host_binary: &Path) -> Result<()> {
    if !host_binary.is_file() {
        return Ok(());
    }
    let output = Command::new("/usr/bin/codesign")
        .arg("-dv")
        .arg("--entitlements")
        .arg(":-")
        .arg(host_binary)
        .output()
        .map_err(|error| SandboxError::io("running codesign for sagens host binary", error))?;
    let mut combined = output.stdout;
    combined.extend_from_slice(&output.stderr);
    let text = String::from_utf8_lossy(&combined);
    if has_hypervisor_entitlement(&text) {
        return Ok(());
    }
    Err(SandboxError::invalid(format!(
        "macOS host binary {} is missing the com.apple.security.hypervisor entitlement; sign it with macos/sagens.entitlements (for example via ./build-local.sh or codesign --force --sign - --entitlements macos/sagens.entitlements --timestamp=none {})",
        host_binary.display(),
        host_binary.display(),
    )))
}

#[cfg(target_os = "macos")]
fn has_hypervisor_entitlement(output: &str) -> bool {
    output.contains("com.apple.security.hypervisor")
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "macos")]
    use super::has_hypervisor_entitlement;

    #[cfg(target_os = "macos")]
    #[test]
    fn detects_hypervisor_entitlement_in_codesign_output() {
        assert!(has_hypervisor_entitlement(
            r#"<plist><dict><key>com.apple.security.hypervisor</key><true/></dict></plist>"#
        ));
        assert!(!has_hypervisor_entitlement(
            "Executable=/tmp/sagens\nSignature=adhoc\n"
        ));
    }
}
