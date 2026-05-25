use std::ffi::CString;
use std::fs;
use std::net::{TcpListener, TcpStream};
use std::os::unix::net::UnixStream;

use crate::{Result, SandboxError};

pub(super) fn verify_security_harness() -> Result<()> {
    if std::process::id() != 1 {
        return Err(SandboxError::backend(format!(
            "secure runner pid namespace check failed: expected pid 1, got {}",
            std::process::id()
        )));
    }
    verify_mount_options("/proc", &["ro", "nosuid", "nodev", "noexec"])?;
    verify_mount_options("/tmp", &["nosuid", "nodev", "noexec"])?;
    verify_dumpable_disabled()?;
    verify_mdwe_if_supported()?;
    expect_failure("reading host file outside secure runner root", || {
        fs::read("/etc/passwd").map(|_| ())
    })?;
    expect_failure("connecting outbound TCP socket from secure runner", || {
        TcpStream::connect(("127.0.0.1", 9)).map(|_| ())
    })?;
    expect_failure("binding TCP listener from secure runner", || {
        TcpListener::bind(("127.0.0.1", 0)).map(|_| ())
    })?;
    expect_failure(
        "executing a new program from secure runner",
        verify_execve_blocked,
    )?;
    expect_failure(
        "connecting arbitrary unix socket from secure runner",
        || UnixStream::connect("/tmp/host.sock").map(|_| ()),
    )?;
    let rc = unsafe { libc::kill(2, 0) };
    if rc == 0 {
        return Err(SandboxError::backend(
            "secure runner unexpectedly signaled pid 2",
        ));
    }
    let error = std::io::Error::last_os_error();
    if error.raw_os_error() != Some(libc::ESRCH) {
        return Err(SandboxError::backend(format!(
            "secure runner signal isolation expected ESRCH for pid 2, got {error}"
        )));
    }
    let fd_count = fs::read_dir("/proc/self/fd")
        .map_err(|error| SandboxError::io("reading secure runner fd table", error))?
        .count();
    if fd_count > 4 {
        return Err(SandboxError::backend(format!(
            "secure runner leaked file descriptors: observed {fd_count}"
        )));
    }
    Ok(())
}

fn expect_failure(label: &str, operation: impl FnOnce() -> std::io::Result<()>) -> Result<()> {
    match operation() {
        Ok(()) => Err(SandboxError::backend(format!(
            "security harness check unexpectedly succeeded: {label}"
        ))),
        Err(_) => Ok(()),
    }
}

fn verify_mount_options(mountpoint: &str, expected: &[&str]) -> Result<()> {
    let mounts = fs::read_to_string("/proc/self/mounts")
        .map_err(|error| SandboxError::io("reading secure runner mount table", error))?;
    let options = mounts
        .lines()
        .find_map(|line| {
            let mut fields = line.split_whitespace();
            let _source = fields.next()?;
            let current_mountpoint = fields.next()?;
            let _fs_type = fields.next()?;
            let options = fields.next()?;
            (current_mountpoint == mountpoint).then_some(options)
        })
        .ok_or_else(|| SandboxError::backend(format!("missing mount entry for {mountpoint}")))?;
    for flag in expected {
        if options.split(',').any(|option| option == *flag) {
            continue;
        }
        return Err(SandboxError::backend(format!(
            "mount {mountpoint} is missing expected option {flag}: {options}"
        )));
    }
    Ok(())
}

fn verify_dumpable_disabled() -> Result<()> {
    let rc = unsafe { libc::prctl(libc::PR_GET_DUMPABLE, 0, 0, 0, 0) };
    if rc == 0 {
        return Ok(());
    }
    if rc < 0 {
        return Err(SandboxError::io(
            "reading PR_GET_DUMPABLE",
            std::io::Error::last_os_error(),
        ));
    }
    Err(SandboxError::backend(format!(
        "secure runner unexpectedly remained dumpable: {rc}"
    )))
}

fn verify_mdwe_if_supported() -> Result<()> {
    let rc = unsafe { libc::prctl(libc::PR_GET_MDWE, 0, 0, 0, 0) };
    if rc < 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::EINVAL) {
            return Ok(());
        }
        return Err(SandboxError::io("reading PR_GET_MDWE", error));
    }
    if (rc as libc::c_uint & libc::PR_MDWE_REFUSE_EXEC_GAIN) != 0 {
        return Ok(());
    }
    Err(SandboxError::backend(
        "secure runner did not retain PR_MDWE_REFUSE_EXEC_GAIN",
    ))
}

fn verify_execve_blocked() -> std::io::Result<()> {
    let path = CString::new("/proc/self/exe")
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid exec path"))?;
    let arg0 = path.as_ptr();
    let argv = [arg0, std::ptr::null()];
    let envp = [std::ptr::null()];
    let rc = unsafe { libc::execve(path.as_ptr(), argv.as_ptr(), envp.as_ptr()) };
    if rc == 0 {
        return Ok(());
    }
    Err(std::io::Error::last_os_error())
}
