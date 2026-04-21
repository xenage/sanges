mod config;
mod instance;
mod loader;
#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
mod qemu_hvf;
pub mod runner;

use std::fs::File;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::mpsc::{RecvTimeoutError, sync_channel};
use std::time::Duration;

use async_trait::async_trait;
use tokio::process::Child;

use crate::backend::{Backend, BackendLaunchOutput, BackendLaunchRequest};
use crate::config::{GuestKernelFormat, IsolationMode};
use crate::guest_transport::GuestTransportEndpoint;
use crate::host_hardening;
use crate::{Result, SandboxError};

const RUNNER_ENV: &str = "SAGENS_LIBKRUN_RUNNER";
const RUNNER_EXE_ENV: &str = "SAGENS_LIBKRUN_RUNNER_EXE";
const PYTHON_RUNNER_MODE: &str = "python-subprocess";
const SELF_RUNNER_MODE: &str = "self-subprocess";
pub(super) const RUNNER_STARTUP_FD_ENV: &str = "SAGENS_LIBKRUN_STARTUP_FD";
const MIN_LINUX_X86_64_RAW_KERNEL_MEMORY_MB: u32 = 3329;
const RECOMMENDED_LINUX_X86_64_RAW_KERNEL_MEMORY_MB: u32 = 3584;

pub struct LibkrunBackend;

#[async_trait]
impl Backend for LibkrunBackend {
    async fn launch(&self, request: BackendLaunchRequest) -> Result<BackendLaunchOutput> {
        validate_launch_request(&request)?;
        let runner_mode = effective_runner_mode(request.isolation_mode);
        if matches!(runner_mode, RunnerMode::Thread) && request.hardening.cgroup_parent.is_some() {
            return Err(SandboxError::invalid(
                "cgroup_parent is not supported by the in-process libkrun backend",
            ));
        }
        match runner_mode {
            RunnerMode::Thread => {}
            RunnerMode::PythonSubprocess => return launch_python_runner(request).await,
            RunnerMode::SelfSubprocess => return launch_self_runner(request).await,
        }
        prepare_runner_artifacts(
            &request.run_layout,
            request.hardening.runner_log_limit_bytes,
        )?;
        let config = config::build_runner_config(&request);
        config::write_debug_runner_config(&request.run_layout.runner_config, &config).await?;
        let runner_log = request.run_layout.runner_log.clone();
        let (started_tx, started_rx) = sync_channel(1);
        let thread_name = format!("libkrun-{}", request.sandbox_id.simple());
        let thread = std::thread::Builder::new()
            .name(thread_name)
            .spawn(move || {
                let result = runner::run_until_exit(config, started_tx);
                if let Err(error) = &result {
                    let _ = std::fs::write(&runner_log, format!("{error}\n"));
                }
                result
            })
            .map_err(|error| SandboxError::io("spawning libkrun runner thread", error))?;
        let shutdown_fd = match started_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(fd)) => fd,
            Ok(Err(error)) => {
                let _ = thread.join();
                return Err(error);
            }
            Err(RecvTimeoutError::Timeout) => {
                return Err(SandboxError::timeout(
                    "timed out waiting for libkrun startup handshake",
                ));
            }
            Err(RecvTimeoutError::Disconnected) => match thread.join() {
                Ok(Ok(())) => {
                    return Err(SandboxError::backend(
                        "libkrun runner exited before reporting startup state",
                    ));
                }
                Ok(Err(error)) => return Err(error),
                Err(_) => {
                    return Err(SandboxError::backend("libkrun runner thread panicked"));
                }
            },
        };
        if thread.is_finished() {
            return match thread.join() {
                Ok(Ok(())) => Err(SandboxError::backend(
                    "libkrun runner exited before guest RPC became ready",
                )),
                Ok(Err(error)) => Err(error),
                Err(_) => Err(SandboxError::backend("libkrun runner thread panicked")),
            };
        }
        Ok(BackendLaunchOutput {
            instance: Arc::new(instance::LibkrunInstance::new_thread(thread, shutdown_fd)),
            guest_endpoint: GuestTransportEndpoint::new(
                request.run_layout.vsock_socket.clone(),
                request.guest.guest_vsock_port,
            ),
        })
    }

    fn name(&self) -> &'static str {
        "libkrun"
    }
}

enum RunnerMode {
    Thread,
    PythonSubprocess,
    SelfSubprocess,
}

fn runner_mode() -> RunnerMode {
    match std::env::var(RUNNER_ENV).ok().as_deref() {
        Some(PYTHON_RUNNER_MODE) => RunnerMode::PythonSubprocess,
        Some(SELF_RUNNER_MODE) => RunnerMode::SelfSubprocess,
        _ if should_use_self_runner_on_macos() => RunnerMode::SelfSubprocess,
        _ => RunnerMode::Thread,
    }
}

fn effective_runner_mode(isolation_mode: IsolationMode) -> RunnerMode {
    if isolation_mode == IsolationMode::Secure {
        return RunnerMode::SelfSubprocess;
    }
    runner_mode()
}

fn validate_launch_request(request: &BackendLaunchRequest) -> Result<()> {
    let Some(min_memory_mb) = min_memory_mb_for_host_kernel(
        std::env::consts::OS,
        std::env::consts::ARCH,
        request.guest.kernel_format,
    ) else {
        return Ok(());
    };
    if request.policy.memory_mb >= min_memory_mb {
        return Ok(());
    }
    Err(SandboxError::invalid(format!(
        "linux-x86_64 libkrun requires at least {min_memory_mb} MiB RAM when booting the raw bundled kernel; set memory_mb to at least {RECOMMENDED_LINUX_X86_64_RAW_KERNEL_MEMORY_MB} MiB"
    )))
}

fn min_memory_mb_for_host_kernel(
    host_os: &str,
    host_arch: &str,
    kernel_format: GuestKernelFormat,
) -> Option<u32> {
    if host_os == "linux" && host_arch == "x86_64" && kernel_format == GuestKernelFormat::Raw {
        return Some(MIN_LINUX_X86_64_RAW_KERNEL_MEMORY_MB);
    }
    None
}

fn should_use_self_runner_on_macos() -> bool {
    #[cfg(target_os = "macos")]
    {
        let Ok(current_exe) = std::env::current_exe() else {
            return false;
        };
        current_exe
            .file_stem()
            .and_then(std::ffi::OsStr::to_str)
            .is_some_and(is_sagens_self_runner_binary)
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

#[cfg(any(target_os = "macos", test))]
fn is_sagens_self_runner_binary(stem: &str) -> bool {
    stem == "sagens" || stem.starts_with("sagens-")
}

async fn launch_python_runner(request: BackendLaunchRequest) -> Result<BackendLaunchOutput> {
    let config = config::build_runner_config(&request);
    prepare_runner_artifacts(
        &request.run_layout,
        request.hardening.runner_log_limit_bytes,
    )?;
    config::write_debug_runner_config(&request.run_layout.runner_config, &config).await?;
    let current_exe = resolve_runner_executable("discovering embedded python executable")?;
    let mut command = tokio::process::Command::new(current_exe);
    command
        .arg("-m")
        .arg("sagens._vm_runner")
        .arg(&request.run_layout.runner_config);
    let child = spawn_runner_process(command, &request, "python").await?;
    Ok(BackendLaunchOutput {
        instance: Arc::new(instance::LibkrunInstance::new_process(child)),
        guest_endpoint: GuestTransportEndpoint::new(
            request.run_layout.vsock_socket.clone(),
            request.guest.guest_vsock_port,
        ),
    })
}

async fn launch_self_runner(request: BackendLaunchRequest) -> Result<BackendLaunchOutput> {
    let config = config::build_runner_config(&request);
    prepare_runner_artifacts(
        &request.run_layout,
        request.hardening.runner_log_limit_bytes,
    )?;
    config::write_debug_runner_config(&request.run_layout.runner_config, &config).await?;
    let current_exe = resolve_runner_executable("discovering sagens executable")?;
    let mut command = tokio::process::Command::new(current_exe);
    command
        .arg("__libkrun-runner")
        .arg(&request.run_layout.runner_config);
    let child = spawn_runner_process(command, &request, "sagens").await?;
    Ok(BackendLaunchOutput {
        instance: Arc::new(instance::LibkrunInstance::new_process(child)),
        guest_endpoint: GuestTransportEndpoint::new(
            request.run_layout.vsock_socket.clone(),
            request.guest.guest_vsock_port,
        ),
    })
}

async fn spawn_runner_process(
    mut command: tokio::process::Command,
    request: &BackendLaunchRequest,
    runner_kind: &str,
) -> Result<Child> {
    let startup_gate = if request.isolation_mode == IsolationMode::Secure {
        Some(create_startup_gate()?)
    } else {
        None
    };
    if let Some(gate) = &startup_gate {
        command.env(RUNNER_STARTUP_FD_ENV, gate.read_end.as_raw_fd().to_string());
    }

    let log_file = open_private_log(&request.run_layout.runner_log)?;
    let stderr = log_file
        .try_clone()
        .map_err(|error| SandboxError::io("cloning libkrun runner log", error))?;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::from(log_file))
        .stderr(Stdio::from(stderr));
    let mut child = command.spawn().map_err(|error| {
        SandboxError::io(format!("spawning {runner_kind} libkrun runner"), error)
    })?;

    let Some(pid) = child.id() else {
        let _ = child.start_kill();
        return Err(SandboxError::backend(
            "spawned libkrun runner did not report a process id",
        ));
    };
    if request.hardening.cgroup_parent.is_some()
        && let Err(error) = host_hardening::attach_backend_process(
            &request.hardening,
            &request.policy,
            request.sandbox_id,
            pid,
        )
        .await
    {
        let _ = child.kill().await;
        let _ = child.wait().await;
        return Err(error);
    }
    if let Some(gate) = startup_gate
        && let Err(error) = release_startup_gate(gate)
    {
        let _ = child.kill().await;
        let _ = child.wait().await;
        return Err(error);
    }
    Ok(child)
}

struct StartupGate {
    read_end: OwnedFd,
    write_end: OwnedFd,
}

fn create_startup_gate() -> Result<StartupGate> {
    let mut fds = [0; 2];
    let rc = unsafe { libc::pipe(fds.as_mut_ptr()) };
    if rc != 0 {
        return Err(SandboxError::io(
            "creating secure runner startup gate",
            std::io::Error::last_os_error(),
        ));
    }
    Ok(StartupGate {
        read_end: unsafe { OwnedFd::from_raw_fd(fds[0]) },
        write_end: unsafe { OwnedFd::from_raw_fd(fds[1]) },
    })
}

fn release_startup_gate(gate: StartupGate) -> Result<()> {
    drop(gate.read_end);
    let byte = [1u8];
    let written = unsafe { libc::write(gate.write_end.as_raw_fd(), byte.as_ptr().cast(), 1) };
    if written != 1 {
        return Err(SandboxError::io(
            "releasing secure runner startup gate",
            std::io::Error::last_os_error(),
        ));
    }
    Ok(())
}

fn prepare_runner_artifacts(
    run_layout: &crate::workspace::RunLayout,
    _runner_log_limit_bytes: u64,
) -> Result<()> {
    if run_layout.vsock_socket.exists() {
        std::fs::remove_file(&run_layout.vsock_socket)
            .map_err(|error| SandboxError::io("removing stale guest rpc socket", error))?;
    }
    let _ = open_private_log(&run_layout.runner_log)?;
    let _ = open_private_log(&run_layout.guest_console_log)?;
    Ok(())
}

fn open_private_log(path: &std::path::Path) -> Result<File> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| SandboxError::io("creating runner log directory", error))?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.create(true).write(true).truncate(true);
    #[cfg(unix)]
    {
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    options
        .open(path)
        .map_err(|error| SandboxError::io(format!("opening {}", path.display()), error))
}

fn resolve_runner_executable(context: &str) -> Result<std::path::PathBuf> {
    if let Some(path) = std::env::var_os(RUNNER_EXE_ENV).filter(|value| !value.is_empty()) {
        return Ok(path.into());
    }
    std::env::current_exe().map_err(|error| SandboxError::io(context, error))
}

#[cfg(test)]
mod tests {
    use super::{is_sagens_self_runner_binary, min_memory_mb_for_host_kernel};
    use crate::config::GuestKernelFormat;

    #[test]
    fn recognizes_packaged_and_local_sagens_binaries() {
        assert!(is_sagens_self_runner_binary("sagens"));
        assert!(is_sagens_self_runner_binary("sagens-local-macos-aarch64"));
        assert!(is_sagens_self_runner_binary("sagens-debug"));
    }

    #[test]
    fn rejects_non_sagens_binary_names() {
        assert!(!is_sagens_self_runner_binary("cargo-test"));
        assert!(!is_sagens_self_runner_binary("agent-box"));
    }

    #[test]
    fn requires_extra_ram_for_linux_x86_64_raw_kernels() {
        assert_eq!(
            min_memory_mb_for_host_kernel("linux", "x86_64", GuestKernelFormat::Raw),
            Some(3329)
        );
    }

    #[test]
    fn skips_extra_ram_guard_for_other_hosts_or_kernel_formats() {
        assert_eq!(
            min_memory_mb_for_host_kernel("linux", "x86_64", GuestKernelFormat::Elf),
            None
        );
        assert_eq!(
            min_memory_mb_for_host_kernel("macos", "x86_64", GuestKernelFormat::Raw),
            None
        );
        assert_eq!(
            min_memory_mb_for_host_kernel("linux", "aarch64", GuestKernelFormat::Raw),
            None
        );
    }
}
