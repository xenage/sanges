use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::guest_rpc::{GuestEvent, decode_bytes, encode_bytes};
use crate::protocol::{ExecExit, ExecRequest, OutputStream, ShellRequest};
use crate::{Result, SandboxError};

use super::linux_boot::BootConfig;
use super::{pty, rpc};

#[derive(Clone)]
pub(crate) struct ShellState {
    pub(crate) master_writer: Arc<Mutex<tokio::fs::File>>,
    pub(crate) child: Arc<Mutex<Child>>,
}

#[derive(Clone)]
pub(crate) struct ExecState {
    child: Arc<Mutex<Child>>,
}

pub(crate) type Execs = Arc<Mutex<HashMap<Uuid, ExecState>>>;

pub(crate) async fn open_shell(
    session_id: Uuid,
    request: ShellRequest,
    config: BootConfig,
    writer: rpc::GuestWriter,
    shells: Arc<Mutex<HashMap<Uuid, ShellState>>>,
) -> Result<()> {
    let pty = pty::open_shell_pty(120, 40)?;
    let slave_fd = pty.slave_fd;
    let stdin = std::process::Stdio::from(
        pty.slave
            .try_clone()
            .map_err(|error| SandboxError::io("cloning shell pty slave for stdin", error))?,
    );
    let stdout = std::process::Stdio::from(
        pty.slave
            .try_clone()
            .map_err(|error| SandboxError::io("cloning shell pty slave for stdout", error))?,
    );
    let stderr = std::process::Stdio::from(pty.slave);
    let mut command = Command::new(&request.program);
    configure_command(
        &mut command,
        &request.args,
        &request.env,
        &request.cwd,
        config,
    )?;
    unsafe {
        command.pre_exec(move || {
            apply_process_limits(
                config.max_processes,
                config.max_open_files,
                config.max_file_size_bytes,
            )?;
            if libc::setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::ioctl(slave_fd, libc::TIOCSCTTY, 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let child = command
        .stdin(stdin)
        .stdout(stdout)
        .stderr(stderr)
        .spawn()
        .map_err(|error| SandboxError::io("spawning shell command", error))?;
    let state = ShellState {
        master_writer: Arc::new(Mutex::new(pty.master_writer)),
        child: Arc::new(Mutex::new(child)),
    };
    shells.lock().await.insert(session_id, state.clone());
    stream_shell_output(session_id, pty.master_reader, writer.clone());
    tokio::spawn(async move {
        let code = wait_child(&state.child).await;
        shells.lock().await.remove(&session_id);
        let _ = rpc::send_event(&writer, &GuestEvent::ShellExit { session_id, code }).await;
    });
    Ok(())
}

pub(crate) fn spawn_exec(
    exec_id: Uuid,
    request: ExecRequest,
    config: BootConfig,
    writer: rpc::GuestWriter,
    execs: Execs,
) -> Result<()> {
    let mut command = Command::new(&request.program);
    configure_command(
        &mut command,
        &request.args,
        &request.env,
        &request.cwd,
        config,
    )?;
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    unsafe {
        command.pre_exec(move || {
            apply_process_limits(
                config.max_processes,
                config.max_open_files,
                config.max_file_size_bytes,
            )?;
            if libc::setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = command
        .spawn()
        .map_err(|error| SandboxError::io("spawning exec command", error))?;
    let mut output_tasks = Vec::new();
    if let Some(stdout) = child.stdout.take() {
        output_tasks.push(stream_exec(
            stdout,
            exec_id,
            OutputStream::Stdout,
            writer.clone(),
        ));
    }
    if let Some(stderr) = child.stderr.take() {
        output_tasks.push(stream_exec(
            stderr,
            exec_id,
            OutputStream::Stderr,
            writer.clone(),
        ));
    }
    let state = ExecState {
        child: Arc::new(Mutex::new(child)),
    };
    let child = state.child.clone();
    tokio::spawn(async move {
        execs.lock().await.insert(exec_id, state);
        let status = wait_exec_status(&child, request.timeout_ms, request.kill_grace_ms).await;
        for task in output_tasks {
            let _ = task.await;
        }
        execs.lock().await.remove(&exec_id);
        let _ = rpc::send_event(&writer, &GuestEvent::ExecExit { exec_id, status }).await;
    });
    Ok(())
}

pub(crate) async fn write_shell_input(shell: &ShellState, data: &str) -> Result<()> {
    shell
        .master_writer
        .lock()
        .await
        .write_all(&decode_bytes(data)?)
        .await
        .map_err(|error| SandboxError::io("writing shell pty input", error))
}

pub(crate) async fn resize_shell(shell: &ShellState, cols: u16, rows: u16) -> Result<()> {
    let master_writer = shell.master_writer.lock().await;
    pty::resize_pty(&master_writer, cols, rows)
}

pub(crate) async fn close_shell(shell: ShellState) {
    terminate_child(&shell.child).await;
}

fn configure_command(
    command: &mut Command,
    args: &[String],
    env: &BTreeMap<String, String>,
    cwd: &str,
    config: BootConfig,
) -> Result<()> {
    let cwd = crate::workspace::resolve_workspace_path(std::path::Path::new("/workspace"), cwd)?;
    command
        .args(args)
        .current_dir(cwd)
        .env("HOME", "/workspace")
        .env("USER", "sandbox")
        .env("TMPDIR", "/tmp")
        .env("NO_PROXY", if config.network_enabled { "" } else { "*" })
        .uid(config.uid)
        .gid(config.gid);
    for (key, value) in env {
        command.env(key, value);
    }
    Ok(())
}

fn apply_process_limits(
    max_processes: u32,
    max_open_files: u32,
    max_file_size_bytes: u64,
) -> std::io::Result<()> {
    apply_rlimit(
        libc::RLIMIT_NPROC,
        max_processes as libc::rlim_t,
        "RLIMIT_NPROC",
    )?;
    apply_rlimit(
        libc::RLIMIT_NOFILE,
        max_open_files as libc::rlim_t,
        "RLIMIT_NOFILE",
    )?;
    apply_rlimit(libc::RLIMIT_CORE, 0, "RLIMIT_CORE")?;
    apply_rlimit(
        libc::RLIMIT_FSIZE,
        max_file_size_bytes as libc::rlim_t,
        "RLIMIT_FSIZE",
    )
}

#[cfg(target_os = "linux")]
type RlimitResource = libc::__rlimit_resource_t;

#[cfg(not(target_os = "linux"))]
type RlimitResource = libc::c_int;

fn apply_rlimit(resource: RlimitResource, value: libc::rlim_t, label: &str) -> std::io::Result<()> {
    let limit = libc::rlimit {
        rlim_cur: value,
        rlim_max: value,
    };
    if unsafe { libc::setrlimit(resource, &limit) } != 0 {
        let error = std::io::Error::last_os_error();
        return Err(std::io::Error::new(
            error.kind(),
            format!("{label}: {error}"),
        ));
    }
    Ok(())
}

async fn wait_exec_status(
    child: &Arc<Mutex<Child>>,
    timeout_ms: Option<u64>,
    kill_grace_ms: u64,
) -> ExecExit {
    if let Some(timeout_ms) = timeout_ms {
        if tokio::time::timeout(Duration::from_millis(timeout_ms), wait_child_exit(child))
            .await
            .is_ok()
        {
            return wait_child_result(child).await;
        }
        if send_process_group_signal(child, libc::SIGTERM)
            .await
            .is_ok()
            && tokio::time::timeout(Duration::from_millis(kill_grace_ms), wait_child_exit(child))
                .await
                .is_ok()
        {
            return ExecExit::Timeout;
        }
        force_kill(child).await;
        let _ = wait_child_exit(child).await;
        return ExecExit::Killed;
    }
    wait_child_result(child).await
}

async fn wait_child_result(child: &Arc<Mutex<Child>>) -> ExecExit {
    match child.lock().await.wait().await {
        Ok(status) if status.success() => ExecExit::Success,
        Ok(status) => ExecExit::ExitCode(status.code().unwrap_or(-1)),
        Err(_) => ExecExit::Killed,
    }
}

async fn wait_child_exit(child: &Arc<Mutex<Child>>) -> Result<()> {
    child
        .lock()
        .await
        .wait()
        .await
        .map(|_| ())
        .map_err(|error| SandboxError::io("waiting for guest child", error))
}

async fn wait_child(child: &Arc<Mutex<Child>>) -> i32 {
    match child.lock().await.wait().await {
        Ok(status) => status.code().unwrap_or(-1),
        Err(_) => -1,
    }
}

async fn terminate_child(child: &Arc<Mutex<Child>>) {
    let _ = send_process_group_signal(child, libc::SIGTERM).await;
    let _ = tokio::time::timeout(Duration::from_millis(250), wait_child_exit(child)).await;
    force_kill(child).await;
}

async fn send_process_group_signal(child: &Arc<Mutex<Child>>, signal: i32) -> Result<()> {
    let pid = child
        .lock()
        .await
        .id()
        .ok_or_else(|| SandboxError::backend("guest child has no pid"))? as i32;
    let rc = unsafe { libc::kill(-pid, signal) };
    if rc == 0 {
        Ok(())
    } else {
        Err(SandboxError::io(
            "sending signal to guest process group",
            std::io::Error::last_os_error(),
        ))
    }
}

async fn force_kill(child: &Arc<Mutex<Child>>) {
    let _ = send_process_group_signal(child, libc::SIGKILL).await;
    let _ = child.lock().await.kill().await;
}

fn stream_exec<R>(
    mut reader: R,
    exec_id: Uuid,
    stream: OutputStream,
    writer: rpc::GuestWriter,
) -> tokio::task::JoinHandle<()>
where
    R: tokio::io::AsyncRead + Send + Unpin + 'static,
{
    tokio::spawn(async move {
        let mut buffer = [0_u8; 1024];
        loop {
            match reader.read(&mut buffer).await {
                Ok(0) | Err(_) => break,
                Ok(size) => {
                    if rpc::send_event(
                        &writer,
                        &GuestEvent::ExecOutput {
                            exec_id,
                            stream,
                            data: encode_bytes(&buffer[..size]),
                        },
                    )
                    .await
                    .is_err()
                    {
                        break;
                    }
                }
            }
        }
    })
}

fn stream_shell_output(
    session_id: Uuid,
    mut master_reader: tokio::fs::File,
    writer: rpc::GuestWriter,
) {
    tokio::spawn(async move {
        let mut buffer = [0_u8; 1024];
        loop {
            match master_reader.read(&mut buffer).await {
                Ok(0) | Err(_) => break,
                Ok(size) => {
                    if rpc::send_event(
                        &writer,
                        &GuestEvent::ShellOutput {
                            session_id,
                            data: encode_bytes(&buffer[..size]),
                        },
                    )
                    .await
                    .is_err()
                    {
                        break;
                    }
                }
            }
        }
    });
}
