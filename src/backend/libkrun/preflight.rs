use std::path::Path;
#[cfg(target_os = "linux")]
use std::process::Stdio;
#[cfg(target_os = "linux")]
use std::time::Duration;

#[cfg(target_os = "linux")]
use crate::config::GuestKernelFormat;
use crate::config::{IsolationMode, RuntimeConfig};
use crate::{Result, SandboxError};

#[cfg(target_os = "linux")]
use super::config;

pub async fn preflight_secure_runner(
    host_binary: &Path,
    runtime_config: &RuntimeConfig,
) -> Result<()> {
    if runtime_config.isolation_mode != IsolationMode::Secure {
        return Ok(());
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = (host_binary, runtime_config);
        Err(SandboxError::UnsupportedHost(
            "secure runner host sandbox preflight requires Linux".into(),
        ))
    }

    #[cfg(target_os = "linux")]
    {
        use tempfile::Builder;

        let harness_root = runtime_config.state_dir.join("secure-preflight");
        tokio::fs::create_dir_all(&harness_root)
            .await
            .map_err(|error| SandboxError::io("creating secure preflight directory", error))?;
        let harness_dir = Builder::new()
            .prefix("runner-")
            .tempdir_in(&harness_root)
            .map_err(|error| SandboxError::io("creating secure preflight tempdir", error))?;
        let kernel_image = harness_dir.path().join("kernel");
        let rootfs_image = harness_dir.path().join("rootfs.raw");
        let workspace_image = harness_dir.path().join("workspace.raw");
        let runtime_dir = harness_dir.path().join("runtime");
        let console_output_path = harness_dir.path().join("guest-console.log");
        let runner_config_path = harness_dir.path().join("runner-config.json");
        for path in [&kernel_image, &rootfs_image, &workspace_image] {
            std::fs::write(path, b"secure-runner-preflight\n")
                .map_err(|error| SandboxError::io(format!("writing {}", path.display()), error))?;
        }
        std::fs::create_dir_all(&runtime_dir)
            .map_err(|error| SandboxError::io("creating secure preflight runtime dir", error))?;

        let runner_config = config::LibkrunRunnerConfig {
            kernel_image,
            kernel_format: GuestKernelFormat::ImageGz,
            rootfs_image,
            cache_image: None,
            workspace_image,
            runtime_dir: runtime_dir.clone(),
            console_output_path,
            firmware: None,
            guest_agent_path: runtime_config.guest.guest_agent_path.clone(),
            cpu_cores: 1,
            memory_mb: 128,
            tmpfs_mib: 64,
            max_processes: 32,
            network_enabled: false,
            guest_uid: runtime_config.guest.guest_uid,
            guest_gid: runtime_config.guest.guest_gid,
            guest_vsock_port: runtime_config.guest.guest_vsock_port.max(1024),
            vsock_socket: runtime_dir.join("guest.sock"),
            isolation_mode: IsolationMode::Secure,
            runner_log_limit_bytes: runtime_config.hardening.runner_log_limit_bytes,
        };
        config::write_debug_runner_config(&runner_config_path, &runner_config).await?;

        let mut command = tokio::process::Command::new(host_binary);
        command
            .arg("__security-harness")
            .arg(&runner_config_path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let output = match tokio::time::timeout(Duration::from_secs(15), command.output()).await {
            Ok(result) => result.map_err(|error| {
                SandboxError::io("waiting for secure runner preflight harness", error)
            })?,
            Err(_) => {
                return Err(SandboxError::timeout(
                    "timed out waiting for secure runner preflight harness",
                ));
            }
        };
        if output.status.success() {
            return Ok(());
        }
        Err(SandboxError::UnsupportedHost(format!(
            "secure runner host sandbox preflight failed: {}",
            format_preflight_output(&output),
        )))
    }
}

#[cfg(any(test, target_os = "linux"))]
fn format_preflight_output(output: &std::process::Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let mut lines = stdout
        .lines()
        .chain(stderr.lines())
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    if lines.len() > 8 {
        lines = lines.split_off(lines.len() - 8);
    }
    if lines.is_empty() {
        return format!("exit_status={}", output.status);
    }
    format!("exit_status={} tail={}", output.status, lines.join(" | "))
}

#[cfg(test)]
mod tests {
    use std::process::Output;

    use super::format_preflight_output;

    #[test]
    fn trims_secure_preflight_output_to_recent_lines() {
        #[cfg(unix)]
        let status = std::os::unix::process::ExitStatusExt::from_raw(1 << 8);
        #[cfg(windows)]
        let status = std::os::windows::process::ExitStatusExt::from_raw(1);
        let summary = format_preflight_output(&Output {
            status,
            stdout: b"one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\n".to_vec(),
            stderr: b"ten\n".to_vec(),
        });
        assert!(summary.contains("three | four | five | six | seven | eight | nine | ten"));
        assert!(!summary.contains("one | two"));
    }
}
