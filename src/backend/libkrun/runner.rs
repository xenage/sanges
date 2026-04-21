use std::env;
#[cfg(target_os = "linux")]
use std::fs;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::path::Path;
use std::sync::mpsc::SyncSender;

use super::RUNNER_STARTUP_FD_ENV;
use super::config::{LibkrunRunnerConfig, read_runner_config};
use super::loader::Libkrun;
#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
use super::qemu_hvf;
use crate::config::IsolationMode;
use crate::{Result, SandboxError};

pub fn run_from_file(path: &Path) -> Result<()> {
    let config = read_runner_config(path)?;
    wait_for_startup_gate()?;
    if config.isolation_mode == IsolationMode::Secure {
        apply_secure_bootstrap(&config)?;
    }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        return qemu_hvf::run(config);
    }
    #[allow(unreachable_code)]
    let libkrun = Libkrun::load(&config.library_path)?;
    let prepared = unsafe { libkrun.prepare_microvm(&config) }?;
    prepared.start_enter()
}

pub fn run_until_exit(
    _config: LibkrunRunnerConfig,
    started_tx: SyncSender<Result<Option<OwnedFd>>>,
) -> Result<()> {
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        let _ = started_tx;
        return Err(SandboxError::UnsupportedHost(
            "the macos-x86_64 HVF backend requires subprocess runner mode".into(),
        ));
    }
    #[allow(unreachable_code)]
    let libkrun = Libkrun::load(&_config.library_path)?;
    let prepared = unsafe { libkrun.prepare_microvm(&_config) }?;
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

fn apply_secure_bootstrap(config: &LibkrunRunnerConfig) -> Result<()> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = config;
        Err(SandboxError::UnsupportedHost(
            "secure runner bootstrap requires Linux".into(),
        ))
    }

    #[cfg(target_os = "linux")]
    {
        unsafe {
            libc::umask(0o077);
        }
        prctl_no_new_privs()?;
        apply_runner_file_limit(config.runner_log_limit_bytes)?;
        isolate_user_namespace()?;
        isolate_mount_and_network()?;
        mount_private_tmp()?;
        // The secure runner performs this bootstrap before starting any threads,
        // so updating process-global temp vars here is safe.
        unsafe {
            env::set_var("TMPDIR", "/tmp");
            env::set_var("TEMP", "/tmp");
            env::set_var("TMP", "/tmp");
        }
        close_extra_fds()?;
        let _ = config;
        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn prctl_no_new_privs() -> Result<()> {
    let rc = unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) };
    if rc != 0 {
        return Err(SandboxError::io(
            "setting PR_SET_NO_NEW_PRIVS",
            std::io::Error::last_os_error(),
        ));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn apply_runner_file_limit(limit_bytes: u64) -> Result<()> {
    let limit = libc::rlimit {
        rlim_cur: limit_bytes as libc::rlim_t,
        rlim_max: limit_bytes as libc::rlim_t,
    };
    if unsafe { libc::setrlimit(libc::RLIMIT_FSIZE, &limit) } != 0 {
        return Err(SandboxError::io(
            "setting runner RLIMIT_FSIZE",
            std::io::Error::last_os_error(),
        ));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn isolate_user_namespace() -> Result<()> {
    let uid = unsafe { libc::geteuid() };
    let gid = unsafe { libc::getegid() };
    syscall_ok(
        unsafe { libc::unshare(libc::CLONE_NEWUSER) },
        "unsharing user namespace",
    )?;
    fs::write("/proc/self/setgroups", "deny\n")
        .map_err(|error| SandboxError::io("writing /proc/self/setgroups", error))?;
    fs::write("/proc/self/uid_map", format!("0 {uid} 1\n"))
        .map_err(|error| SandboxError::io("writing /proc/self/uid_map", error))?;
    fs::write("/proc/self/gid_map", format!("0 {gid} 1\n"))
        .map_err(|error| SandboxError::io("writing /proc/self/gid_map", error))?;
    syscall_ok(
        unsafe { libc::setresgid(0, 0, 0) },
        "switching secure runner gid mapping",
    )?;
    syscall_ok(
        unsafe { libc::setresuid(0, 0, 0) },
        "switching secure runner uid mapping",
    )?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn isolate_mount_and_network() -> Result<()> {
    syscall_ok(
        unsafe { libc::unshare(libc::CLONE_NEWNS | libc::CLONE_NEWNET) },
        "unsharing mount and network namespaces",
    )?;
    mount(
        None,
        "/",
        None,
        (libc::MS_REC | libc::MS_PRIVATE) as libc::c_ulong,
        None,
        "making mount namespace private",
    )
}

#[cfg(target_os = "linux")]
fn mount_private_tmp() -> Result<()> {
    let _ = fs::create_dir_all("/tmp");
    mount(
        Some("tmpfs"),
        "/tmp",
        Some("tmpfs"),
        libc::MS_NOSUID as libc::c_ulong | libc::MS_NODEV as libc::c_ulong,
        Some("mode=700,size=16777216"),
        "mounting private /tmp",
    )
}

#[cfg(target_os = "linux")]
fn close_extra_fds() -> Result<()> {
    let mut to_close = Vec::new();
    for entry in fs::read_dir("/proc/self/fd")
        .map_err(|error| SandboxError::io("reading /proc/self/fd", error))?
    {
        let entry =
            entry.map_err(|error| SandboxError::io("reading /proc/self/fd entry", error))?;
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let Ok(fd) = name.parse::<RawFd>() else {
            continue;
        };
        if fd > 2 {
            to_close.push(fd);
        }
    }
    for fd in to_close {
        unsafe {
            libc::close(fd);
        }
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn mount(
    source: Option<&str>,
    target: &str,
    filesystem_type: Option<&str>,
    flags: libc::c_ulong,
    data: Option<&str>,
    context: &str,
) -> Result<()> {
    let source = source
        .map(std::ffi::CString::new)
        .transpose()
        .map_err(|_| SandboxError::invalid(format!("{context}: source contains interior NUL")))?;
    let target = std::ffi::CString::new(target)
        .map_err(|_| SandboxError::invalid(format!("{context}: target contains interior NUL")))?;
    let filesystem_type = filesystem_type
        .map(std::ffi::CString::new)
        .transpose()
        .map_err(|_| {
            SandboxError::invalid(format!("{context}: filesystem type contains interior NUL"))
        })?;
    let data = data
        .map(std::ffi::CString::new)
        .transpose()
        .map_err(|_| SandboxError::invalid(format!("{context}: data contains interior NUL")))?;
    let rc = unsafe {
        libc::mount(
            source
                .as_ref()
                .map_or(std::ptr::null(), |value| value.as_ptr()),
            target.as_ptr(),
            filesystem_type
                .as_ref()
                .map_or(std::ptr::null(), |value| value.as_ptr()),
            flags,
            data.as_ref()
                .map_or(std::ptr::null(), |value| value.as_ptr().cast()),
        )
    };
    syscall_ok(rc, context)
}

#[cfg(target_os = "linux")]
fn syscall_ok(rc: libc::c_int, context: &str) -> Result<()> {
    if rc != 0 {
        return Err(SandboxError::io(context, std::io::Error::last_os_error()));
    }
    Ok(())
}
