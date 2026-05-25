use std::ffi::CString;
use std::os::fd::{FromRawFd, OwnedFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use crate::{Result, SandboxError};

pub(super) fn bpf_stmt(code: u16, k: u32) -> libc::sock_filter {
    libc::sock_filter {
        code,
        jt: 0,
        jf: 0,
        k,
    }
}

pub(super) fn bpf_jump(code: u16, k: u32, jt: u8, jf: u8) -> libc::sock_filter {
    libc::sock_filter { code, jt, jf, k }
}

pub(super) fn seccomp_audit_arch() -> u32 {
    #[cfg(target_arch = "x86_64")]
    {
        0xc000_003e
    }
    #[cfg(target_arch = "aarch64")]
    {
        0xc000_00b7
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        0
    }
}

pub(super) fn path_cstring(path: &Path, context: &str) -> Result<CString> {
    CString::new(path.as_os_str().as_bytes()).map_err(|_| {
        SandboxError::invalid(format!(
            "{context}: path contains interior NUL: {}",
            path.display()
        ))
    })
}

pub(super) fn mount(
    source: Option<&str>,
    target: &str,
    filesystem_type: Option<&str>,
    flags: libc::c_ulong,
    data: Option<&str>,
    context: &str,
) -> Result<()> {
    mount_path(
        source.map(Path::new),
        Path::new(target),
        filesystem_type,
        flags,
        data,
        context,
    )
}

pub(super) fn mount_path(
    source: Option<&Path>,
    target: &Path,
    filesystem_type: Option<&str>,
    flags: libc::c_ulong,
    data: Option<&str>,
    context: &str,
) -> Result<()> {
    let source = source
        .map(|value| path_cstring(value, context))
        .transpose()?;
    let target = path_cstring(target, context)?;
    let filesystem_type = filesystem_type.map(CString::new).transpose().map_err(|_| {
        SandboxError::invalid(format!("{context}: filesystem type contains interior NUL"))
    })?;
    let data = data
        .map(CString::new)
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

pub(super) fn syscall_ok(rc: libc::c_int, context: &str) -> Result<()> {
    if rc != 0 {
        return Err(SandboxError::io(context, std::io::Error::last_os_error()));
    }
    Ok(())
}

pub(super) fn syscall_owned_fd(rc: libc::c_long, context: impl Into<String>) -> Result<OwnedFd> {
    if rc < 0 {
        return Err(SandboxError::io(
            context.into(),
            std::io::Error::last_os_error(),
        ));
    }
    Ok(unsafe { OwnedFd::from_raw_fd(rc as RawFd) })
}
