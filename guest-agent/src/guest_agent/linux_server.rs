use std::collections::HashMap;
use std::sync::Arc;

use crate::guest_rpc::{GuestEvent, GuestRequest, GuestRpcReady, ReadFilePayload, decode_bytes};
use crate::{Result, SandboxError};
use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;
use tokio::sync::Mutex;
use tokio_vsock::{VMADDR_CID_ANY, VsockAddr, VsockListener, VsockStream};
use uuid::Uuid;

use super::linux_boot::{BootConfig, RpcTransport};
use super::linux_exec::{self, ShellState};
use super::{bootstrap, fs, rpc, stats};

pub(crate) fn entry() {
    runtime_entry();
}

#[tokio::main]
async fn runtime_entry() {
    if let Err(error) = run().await {
        eprintln!("guest agent error: {error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    eprintln!("guest agent: starting boot sequence");
    bootstrap::mount_runtime_filesystems()?;
    let boot = BootConfig::from_cmdline("/proc/cmdline")?;
    eprintln!(
        "guest agent: boot config rpc_port={} rpc_transport={:?} tmpfs_mib={} uid={} gid={} max_processes={} network_enabled={}",
        boot.rpc_port,
        boot.rpc_transport,
        boot.tmpfs_mib,
        boot.uid,
        boot.gid,
        boot.max_processes,
        boot.network_enabled
    );
    bootstrap::bootstrap_guest(boot)?;
    bootstrap::append_boot_log("boot config parsed\n");
    match boot.rpc_transport {
        RpcTransport::Vsock => {
            let listener = VsockListener::bind(VsockAddr::new(VMADDR_CID_ANY, boot.rpc_port))
                .map_err(|error| SandboxError::io("binding guest vsock listener", error))?;
            bootstrap::append_boot_log("vsock listener bound\n");
            eprintln!(
                "guest agent: vsock listener bound on port {}",
                boot.rpc_port
            );
            loop {
                let (stream, _) = listener
                    .accept()
                    .await
                    .map_err(|error| SandboxError::io("accepting guest vsock connection", error))?;
                eprintln!("guest agent: accepted vsock connection");
                if handle_connection(stream, boot).await? {
                    break;
                }
            }
        }
        RpcTransport::VirtioSerial => {
            let path = "/dev/virtio-ports/sagens-rpc";
            bootstrap::append_boot_log("opening virtio-serial rpc device\n");
            let stream = open_virtio_serial(path).await?;
            eprintln!("guest agent: virtio-serial rpc device ready at {path}");
            let _ = handle_connection(stream, boot).await?;
        }
    }
    Ok(())
}

async fn open_virtio_serial(path: &str) -> Result<tokio::fs::File> {
    let start = std::time::Instant::now();
    loop {
        match tokio::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .await
        {
            Ok(file) => return Ok(file),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(SandboxError::io(
                    format!("opening guest virtio-serial device {path}"),
                    error,
                ));
            }
        }
        if start.elapsed() > std::time::Duration::from_secs(10) {
            return Err(SandboxError::timeout(format!(
                "timed out waiting for guest virtio-serial device {path}"
            )));
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

async fn handle_connection<T>(stream: T, config: BootConfig) -> Result<bool>
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let (reader, writer) = tokio::io::split(stream);
    let writer = Arc::new(Mutex::new(rpc::box_writer(writer)));
    rpc::send_event(
        &writer,
        &GuestEvent::Ready {
            ready: GuestRpcReady {
                protocol_version: 3,
                capabilities: vec![
                    "exec".into(),
                    "shell".into(),
                    "runtime-stats".into(),
                    "workspace-files".into(),
                    "workspace-snapshot".into(),
                ],
            },
        },
    )
    .await?;

    let mut lines = BufReader::new(rpc::box_reader(reader)).lines();
    let shells = Arc::new(Mutex::new(HashMap::<Uuid, ShellState>::new()));
    let execs = Arc::new(Mutex::new(HashMap::new()));
    while let Some(request) = rpc::next_request(&mut lines).await? {
        match request {
            GuestRequest::Ping { request_id } => {
                rpc::send_event(&writer, &GuestEvent::Pong { request_id }).await?;
            }
            GuestRequest::Exec {
                request_id,
                exec_id,
                request,
            } => {
                if let Err(error) =
                    linux_exec::spawn_exec(exec_id, request, config, writer.clone(), execs.clone())
                {
                    send_error(&writer, Some(request_id), Some(exec_id), &error).await?;
                }
            }
            GuestRequest::OpenShell {
                request_id,
                session_id,
                request,
            } => {
                match linux_exec::open_shell(
                    session_id,
                    request,
                    config,
                    writer.clone(),
                    shells.clone(),
                )
                .await
                {
                    Ok(()) => {
                        rpc::send_event(
                            &writer,
                            &GuestEvent::ShellOpened {
                                request_id,
                                session_id,
                            },
                        )
                        .await?;
                    }
                    Err(error) => {
                        send_error(&writer, Some(request_id), Some(session_id), &error).await?;
                    }
                }
            }
            GuestRequest::ShellInput {
                request_id,
                session_id,
                data,
            } => match shell_state(&shells, session_id).await {
                Ok(shell) => {
                    linux_exec::write_shell_input(&shell, &data).await?;
                    rpc::send_event(&writer, &GuestEvent::Ack { request_id }).await?;
                }
                Err(error) => {
                    send_error(&writer, Some(request_id), Some(session_id), &error).await?
                }
            },
            GuestRequest::ResizeShell {
                request_id,
                session_id,
                cols,
                rows,
            } => match shell_state(&shells, session_id).await {
                Ok(shell) => {
                    linux_exec::resize_shell(&shell, cols, rows).await?;
                    rpc::send_event(&writer, &GuestEvent::Ack { request_id }).await?;
                }
                Err(error) => {
                    send_error(&writer, Some(request_id), Some(session_id), &error).await?
                }
            },
            GuestRequest::CloseShell {
                request_id,
                session_id,
            } => match shells.lock().await.remove(&session_id) {
                Some(shell) => {
                    linux_exec::close_shell(shell).await;
                    rpc::send_event(&writer, &GuestEvent::Ack { request_id }).await?;
                }
                None => {
                    send_error(
                        &writer,
                        Some(request_id),
                        Some(session_id),
                        &SandboxError::invalid(format!("unknown shell session {session_id}")),
                    )
                    .await?;
                }
            },
            GuestRequest::SnapshotWorkspace { request_id } => {
                match fs::snapshot_workspace().await {
                    Ok(entries) => {
                        rpc::send_event(
                            &writer,
                            &GuestEvent::WorkspaceSnapshot {
                                request_id,
                                entries,
                            },
                        )
                        .await?;
                    }
                    Err(error) => send_error(&writer, Some(request_id), None, &error).await?,
                }
            }
            GuestRequest::SyncWorkspace { request_id } => match fs::sync_workspace().await {
                Ok(()) => rpc::send_event(&writer, &GuestEvent::Ack { request_id }).await?,
                Err(error) => send_error(&writer, Some(request_id), None, &error).await?,
            },
            GuestRequest::RuntimeStats { request_id } => match stats::runtime_stats().await {
                Ok(stats) => {
                    rpc::send_event(&writer, &GuestEvent::RuntimeStats { request_id, stats })
                        .await?;
                }
                Err(error) => send_error(&writer, Some(request_id), None, &error).await?,
            },
            GuestRequest::ListFiles { request_id, path } => match fs::list_files(&path).await {
                Ok(entries) => {
                    rpc::send_event(
                        &writer,
                        &GuestEvent::FilesListed {
                            request_id,
                            entries,
                        },
                    )
                    .await?;
                }
                Err(error) => send_error(&writer, Some(request_id), None, &error).await?,
            },
            GuestRequest::ReadFile {
                request_id,
                path,
                limit,
            } => match fs::read_file(&path, limit).await {
                Ok(file) => {
                    let file = ReadFilePayload::from_read_file(&file);
                    rpc::send_event(&writer, &GuestEvent::FileRead { request_id, file }).await?;
                }
                Err(error) => send_error(&writer, Some(request_id), None, &error).await?,
            },
            GuestRequest::WriteFile {
                request_id,
                path,
                data,
                create_parents,
            } => match decode_bytes(&data) {
                Ok(data) => match fs::write_file(&path, data, create_parents).await {
                    Ok(()) => rpc::send_event(&writer, &GuestEvent::Ack { request_id }).await?,
                    Err(error) => send_error(&writer, Some(request_id), None, &error).await?,
                },
                Err(error) => send_error(&writer, Some(request_id), None, &error).await?,
            },
            GuestRequest::MakeDir {
                request_id,
                path,
                recursive,
            } => match fs::make_dir(&path, recursive).await {
                Ok(()) => rpc::send_event(&writer, &GuestEvent::Ack { request_id }).await?,
                Err(error) => send_error(&writer, Some(request_id), None, &error).await?,
            },
            GuestRequest::RemovePath {
                request_id,
                path,
                recursive,
            } => match fs::remove_path(&path, recursive).await {
                Ok(()) => rpc::send_event(&writer, &GuestEvent::Ack { request_id }).await?,
                Err(error) => send_error(&writer, Some(request_id), None, &error).await?,
            },
            GuestRequest::Shutdown { request_id } => {
                for (_, shell) in shells.lock().await.drain() {
                    linux_exec::close_shell(shell).await;
                }
                rpc::send_event(&writer, &GuestEvent::Ack { request_id }).await?;
                return Ok(true);
            }
        }
    }
    Ok(false)
}

async fn shell_state(
    shells: &Arc<Mutex<HashMap<Uuid, ShellState>>>,
    session_id: Uuid,
) -> Result<ShellState> {
    shells
        .lock()
        .await
        .get(&session_id)
        .cloned()
        .ok_or_else(|| SandboxError::invalid(format!("unknown shell session {session_id}")))
}

async fn send_error(
    writer: &rpc::GuestWriter,
    request_id: Option<String>,
    target: Option<Uuid>,
    error: &SandboxError,
) -> Result<()> {
    rpc::send_event(
        writer,
        &GuestEvent::Error {
            request_id,
            target,
            message: error.to_string(),
        },
    )
    .await
}
