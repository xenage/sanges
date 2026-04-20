use std::os::fd::{AsRawFd, OwnedFd};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use async_trait::async_trait;
use tokio::process::Child;
use tokio::sync::Mutex;

use crate::backend::{BackendCapabilities, BackendInstance};
use crate::{Result, SandboxError};

enum RunningVm {
    Thread {
        thread: JoinHandle<Result<()>>,
        shutdown_fd: OwnedFd,
    },
    Process {
        child: Child,
    },
}

pub struct LibkrunInstance {
    running: Arc<Mutex<Option<RunningVm>>>,
}

impl LibkrunInstance {
    pub fn new_thread(thread: JoinHandle<Result<()>>, shutdown_fd: OwnedFd) -> Self {
        Self {
            running: Arc::new(Mutex::new(Some(RunningVm::Thread {
                thread,
                shutdown_fd,
            }))),
        }
    }

    pub fn new_process(child: Child) -> Self {
        Self {
            running: Arc::new(Mutex::new(Some(RunningVm::Process { child }))),
        }
    }
}

#[async_trait]
impl BackendInstance for LibkrunInstance {
    async fn shutdown(&self) -> Result<()> {
        let Some(running) = self.running.lock().await.take() else {
            return Ok(());
        };
        match running {
            RunningVm::Thread {
                thread,
                shutdown_fd,
            } => shutdown_thread_vm(thread, shutdown_fd).await,
            RunningVm::Process { child } => shutdown_process_vm(child).await,
        }
    }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            supports_graceful_shutdown: true,
            supports_vsock: true,
        }
    }
}

async fn shutdown_thread_vm(thread: JoinHandle<Result<()>>, shutdown_fd: OwnedFd) -> Result<()> {
    if !thread.is_finished() {
        signal_shutdown(&shutdown_fd)?;
    }
    let joined = tokio::task::spawn_blocking(move || thread.join());
    match tokio::time::timeout(Duration::from_secs(5), joined).await {
        Ok(Ok(Ok(result))) => result,
        Ok(Ok(Err(_))) => Err(SandboxError::backend("libkrun runner thread panicked")),
        Ok(Err(error)) => Err(SandboxError::backend(format!(
            "joining libkrun runner thread task failed: {error}"
        ))),
        Err(_) => Err(SandboxError::timeout(
            "timed out waiting for libkrun runner thread shutdown",
        )),
    }
}

async fn shutdown_process_vm(mut child: Child) -> Result<()> {
    if tokio::time::timeout(Duration::from_secs(2), child.wait())
        .await
        .is_ok()
    {
        return Ok(());
    }
    child
        .kill()
        .await
        .map_err(|error| SandboxError::io("killing libkrun runner process", error))?;
    let _ = child
        .wait()
        .await
        .map_err(|error| SandboxError::io("waiting for libkrun runner process exit", error))?;
    Ok(())
}

fn signal_shutdown(fd: &OwnedFd) -> Result<()> {
    let value = 1u64.to_ne_bytes();
    let written = unsafe { libc::write(fd.as_raw_fd(), value.as_ptr().cast(), value.len()) };
    if written < 0 {
        return Err(SandboxError::io(
            "writing libkrun shutdown eventfd",
            std::io::Error::last_os_error(),
        ));
    }
    Ok(())
}
