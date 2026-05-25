#[path = "runner/secure_runner/mod.rs"]
mod secure_runner;

use std::env;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::path::Path;
use std::sync::mpsc::SyncSender;

use super::RUNNER_STARTUP_FD_ENV;
use super::config::{LibkrunRunnerConfig, read_runner_config};
use super::loader::Libkrun;
use crate::config::IsolationMode;
use crate::{Result, SandboxError};

pub fn run_from_file(path: &Path) -> Result<()> {
    let mut config = read_runner_config(path)?;
    config.validate_secure_constraints()?;
    wait_for_startup_gate()?;
    if config.isolation_mode == IsolationMode::Secure {
        secure_runner::apply_secure_bootstrap(&mut config)?;
    }
    let prepared = unsafe { Libkrun::new().prepare_microvm(&config) }?;
    prepared.start_enter()
}

pub fn run_security_harness_from_file(path: &Path) -> Result<()> {
    let mut config = read_runner_config(path)?;
    config.validate_secure_constraints()?;
    wait_for_startup_gate()?;
    if config.isolation_mode != IsolationMode::Secure {
        return Err(SandboxError::invalid(
            "security harness requires secure isolation mode",
        ));
    }
    secure_runner::run_security_harness(&mut config)
}

pub fn run_until_exit(
    config: LibkrunRunnerConfig,
    started_tx: SyncSender<Result<Option<OwnedFd>>>,
) -> Result<()> {
    let prepared = unsafe { Libkrun::new().prepare_microvm(&config) }?;
    let shutdown_fd = prepared.shutdown_fd().map(duplicate_fd).transpose()?;
    let _ = started_tx.send(Ok(shutdown_fd));
    prepared.start_enter()
}

fn duplicate_fd(raw_fd: RawFd) -> Result<OwnedFd> {
    let duplicated = unsafe { libc::dup(raw_fd) };
    if duplicated < 0 {
        return Err(SandboxError::io(
            "duplicating libkrun shutdown eventfd",
            std::io::Error::last_os_error(),
        ));
    }
    Ok(unsafe { OwnedFd::from_raw_fd(duplicated) })
}

fn wait_for_startup_gate() -> Result<()> {
    let Some(raw_fd) = env::var(RUNNER_STARTUP_FD_ENV).ok() else {
        return Ok(());
    };
    let fd_num = raw_fd.parse::<RawFd>().map_err(|error| {
        SandboxError::invalid(format!(
            "invalid {RUNNER_STARTUP_FD_ENV} value {raw_fd}: {error}"
        ))
    })?;
    let fd = unsafe { OwnedFd::from_raw_fd(fd_num) };
    let mut byte = [0u8; 1];
    loop {
        let read = unsafe { libc::read(fd.as_raw_fd(), byte.as_mut_ptr().cast(), byte.len()) };
        if read == 1 {
            return Ok(());
        }
        if read == 0 {
            return Err(SandboxError::backend(
                "secure runner startup gate closed before release",
            ));
        }
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::Interrupted {
            continue;
        }
        return Err(SandboxError::io(
            "waiting for secure runner startup gate",
            error,
        ));
    }
}
