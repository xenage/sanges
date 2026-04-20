use std::fs;
use std::io;

use crate::{Result, SandboxError};

use super::linux_boot::BootConfig;

pub(crate) fn mount_runtime_filesystems() -> Result<()> {
    eprintln!("guest bootstrap: mounting proc/sys/dev");
    ensure_dir("/proc")?;
    ensure_dir("/sys")?;
    ensure_dir("/dev")?;
    ensure_dir("/dev/pts")?;
    mount_fs("proc", "/proc", "proc", 0, None)?;
    mount_fs("sysfs", "/sys", "sysfs", 0, None)?;
    mount_fs("devtmpfs", "/dev", "devtmpfs", 0, Some("mode=0755"))?;
    mount_fs(
        "devpts",
        "/dev/pts",
        "devpts",
        0,
        Some("mode=0620,ptmxmode=0666"),
    )?;
    Ok(())
}

pub(crate) fn bootstrap_guest(config: BootConfig) -> Result<()> {
    eprintln!("guest bootstrap: mounting tmpfs and workspace");
    mount_fs(
        "tmpfs",
        "/tmp",
        "tmpfs",
        libc::MS_NOSUID | libc::MS_NODEV,
        Some(&format!("size={}m,mode=1777", config.tmpfs_mib)),
    )?;
    let workspace_device = read_workspace_device("/proc/cmdline")?;
    mount_fs(&workspace_device, "/workspace", "ext4", 0, None)?;
    append_boot_log("workspace mounted\n");
    eprintln!("guest bootstrap: workspace mounted from {workspace_device}");
    chown_path("/workspace", config.uid, config.gid)?;
    Ok(())
}

pub(crate) fn append_boot_log(line: &str) {
    let _ignored = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/workspace/boot.log")
        .and_then(|mut file| std::io::Write::write_all(&mut file, line.as_bytes()));
}

fn read_workspace_device(path: &str) -> Result<String> {
    let cmdline = std::fs::read_to_string(path)
        .map_err(|error| SandboxError::io("reading /proc/cmdline", error))?;
    Ok(cmdline
        .split_whitespace()
        .find_map(|token: &str| {
            token
                .strip_prefix("sandbox.workspace_device=")
                .map(str::to_string)
        })
        .unwrap_or_else(|| "/dev/vdb".to_string()))
}

fn chown_path(path: &str, uid: u32, gid: u32) -> Result<()> {
    let c_path = make_cstring(path, "workspace path")?;
    let result = unsafe { libc::chown(c_path.as_ptr(), uid, gid) };
    if result == 0 {
        Ok(())
    } else {
        Err(SandboxError::io(
            "chown workspace",
            io::Error::last_os_error(),
        ))
    }
}

fn mount_fs(
    source: &str,
    target: &str,
    fstype: &str,
    flags: libc::c_ulong,
    data: Option<&str>,
) -> Result<()> {
    if fs::metadata(target).is_err() {
        return Err(SandboxError::invalid(format!(
            "missing mountpoint {target} in guest image"
        )));
    }

    let source_c = make_cstring(source, "mount source")?;
    let target_c = make_cstring(target, "mount target")?;
    let fstype_c = make_cstring(fstype, "mount filesystem type")?;
    let data_c = data
        .map(|value| make_cstring(value, "mount data"))
        .transpose()?;
    let data_ptr = data_c
        .as_ref()
        .map_or(std::ptr::null(), |value: &std::ffi::CString| {
            value.as_ptr() as *const libc::c_void
        });

    let result = unsafe {
        libc::mount(
            source_c.as_ptr(),
            target_c.as_ptr(),
            fstype_c.as_ptr(),
            flags,
            data_ptr,
        )
    };
    if result == 0 {
        return Ok(());
    }

    match io::Error::last_os_error().raw_os_error() {
        Some(libc::EBUSY) => Ok(()),
        _ => Err(SandboxError::io(
            format!("mounting {source} on {target}"),
            io::Error::last_os_error(),
        )),
    }
}

fn ensure_dir(path: &str) -> Result<()> {
    fs::create_dir_all(path).map_err(|error| SandboxError::io(format!("creating {path}"), error))
}

fn make_cstring(value: &str, field: &str) -> Result<std::ffi::CString> {
    std::ffi::CString::new(value)
        .map_err(|_| SandboxError::invalid(format!("{field} contains a null byte")))
}
