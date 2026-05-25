use std::env;
use std::ffi::CString;
use std::fs;
use std::os::fd::RawFd;
use std::path::{Path, PathBuf};

use crate::backend::libkrun::config::LibkrunRunnerConfig;
use crate::{Result, SandboxError};

use super::policies::{apply_landlock_policy, apply_seccomp_filter};
use super::sys::{mount, mount_path, path_cstring, syscall_ok};

pub(super) fn apply_secure_bootstrap(config: &mut LibkrunRunnerConfig) -> Result<()> {
    unsafe {
        libc::umask(0o077);
    }
    ensure_runtime_root(config)?;
    prctl_no_new_privs()?;
    prctl_disable_dumpable()?;
    apply_memory_deny_write_execute()?;
    apply_runner_file_limit(config.runner_log_limit_bytes)?;
    apply_runner_fd_limit(config.max_processes)?;
    isolate_user_namespace()?;
    isolate_mount_ipc_uts_network_and_cgroup()?;
    let root_dir = stage_secure_runtime_root(config)?;
    enter_pid_namespace()?;
    enter_chroot(&root_dir)?;
    mount_proc()?;
    mount_private_tmp()?;
    unsafe {
        env::set_var("TMPDIR", "/tmp");
        env::set_var("TEMP", "/tmp");
        env::set_var("TMP", "/tmp");
    }
    close_extra_fds()?;
    apply_landlock_policy()?;
    apply_seccomp_filter()?;
    Ok(())
}

fn ensure_runtime_root(config: &LibkrunRunnerConfig) -> Result<()> {
    fs::create_dir_all(&config.runtime_dir)
        .map_err(|error| SandboxError::io("creating secure runner runtime root", error))
}

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

fn prctl_disable_dumpable() -> Result<()> {
    let rc = unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) };
    if rc != 0 {
        return Err(SandboxError::io(
            "setting PR_SET_DUMPABLE",
            std::io::Error::last_os_error(),
        ));
    }
    Ok(())
}

fn apply_memory_deny_write_execute() -> Result<()> {
    let rc = unsafe {
        libc::prctl(
            libc::PR_SET_MDWE,
            libc::PR_MDWE_REFUSE_EXEC_GAIN as libc::c_ulong,
            0,
            0,
            0,
        )
    };
    if rc == 0 {
        return Ok(());
    }
    let error = std::io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::EINVAL) {
        return Ok(());
    }
    Err(SandboxError::io("setting PR_SET_MDWE", error))
}

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

fn apply_runner_fd_limit(max_processes: u32) -> Result<()> {
    let max_open_files = max_processes.saturating_mul(16).clamp(256, 4096) as u64;
    let limit = libc::rlimit {
        rlim_cur: max_open_files as libc::rlim_t,
        rlim_max: max_open_files as libc::rlim_t,
    };
    if unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &limit) } != 0 {
        return Err(SandboxError::io(
            "setting runner RLIMIT_NOFILE",
            std::io::Error::last_os_error(),
        ));
    }
    Ok(())
}

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

fn isolate_mount_ipc_uts_network_and_cgroup() -> Result<()> {
    syscall_ok(
        unsafe {
            libc::unshare(
                libc::CLONE_NEWNS
                    | libc::CLONE_NEWNET
                    | libc::CLONE_NEWIPC
                    | libc::CLONE_NEWUTS
                    | libc::CLONE_NEWCGROUP,
            )
        },
        "unsharing mount, network, ipc, uts, and cgroup namespaces",
    )?;
    mount_path(
        None,
        Path::new("/"),
        None,
        (libc::MS_REC | libc::MS_PRIVATE) as libc::c_ulong,
        None,
        "making mount namespace private",
    )
}

fn stage_secure_runtime_root(config: &mut LibkrunRunnerConfig) -> Result<PathBuf> {
    let root_dir = secure_root_dir(config)?;
    let guest_assets_dir = root_dir.join("guest-assets");
    let dev_dir = root_dir.join("dev");
    let proc_dir = root_dir.join("proc");
    let tmp_dir = root_dir.join("tmp");
    for dir in [
        &root_dir,
        &config.runtime_dir,
        &guest_assets_dir,
        &dev_dir,
        &proc_dir,
        &tmp_dir,
    ] {
        fs::create_dir_all(dir)
            .map_err(|error| SandboxError::io(format!("creating {}", dir.display()), error))?;
    }

    bind_mount_path(
        &config.kernel_image,
        &guest_assets_dir.join("kernel"),
        true,
        "staging secure runner kernel image",
    )?;
    bind_mount_path(
        &config.rootfs_image,
        &guest_assets_dir.join("rootfs.raw"),
        true,
        "staging secure runner rootfs image",
    )?;
    if let Some(cache_image) = &config.cache_image {
        bind_mount_path(
            cache_image,
            &guest_assets_dir.join("cache.raw"),
            true,
            "staging secure runner cache image",
        )?;
    }
    bind_mount_path(
        &config.workspace_image,
        &guest_assets_dir.join("workspace.raw"),
        false,
        "staging secure runner workspace image",
    )?;
    if let Some(firmware) = &config.firmware {
        bind_mount_path(
            firmware,
            &guest_assets_dir.join("firmware.fd"),
            true,
            "staging secure runner firmware image",
        )?;
    }
    for (source, name) in [
        (Path::new("/dev/kvm"), "kvm"),
        (Path::new("/dev/null"), "null"),
        (Path::new("/dev/zero"), "zero"),
        (Path::new("/dev/urandom"), "urandom"),
    ] {
        bind_mount_path(
            source,
            &dev_dir.join(name),
            false,
            &format!("staging secure runner device {name}"),
        )?;
    }

    config.kernel_image = PathBuf::from("/guest-assets/kernel");
    config.rootfs_image = PathBuf::from("/guest-assets/rootfs.raw");
    if config.cache_image.is_some() {
        config.cache_image = Some(PathBuf::from("/guest-assets/cache.raw"));
    }
    config.workspace_image = PathBuf::from("/guest-assets/workspace.raw");
    config.runtime_dir = PathBuf::from("/runtime");
    config.console_output_path = PathBuf::from("/guest-console.log");
    config.vsock_socket = PathBuf::from("/runtime/guest.sock");
    if config.firmware.is_some() {
        config.firmware = Some(PathBuf::from("/guest-assets/firmware.fd"));
    }
    Ok(root_dir)
}

fn secure_root_dir(config: &LibkrunRunnerConfig) -> Result<PathBuf> {
    config
        .runtime_dir
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| SandboxError::invalid("secure runner runtime_dir must have a parent"))
}

fn bind_mount_path(source: &Path, target: &Path, read_only: bool, context: &str) -> Result<()> {
    if !source.exists() {
        return Err(SandboxError::UnsupportedHost(format!(
            "{context}: missing {}",
            source.display()
        )));
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            SandboxError::io(format!("{context}: creating {}", parent.display()), error)
        })?;
    }
    if source.is_dir() {
        fs::create_dir_all(target).map_err(|error| {
            SandboxError::io(format!("{context}: creating {}", target.display()), error)
        })?;
    } else {
        fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(target)
            .map_err(|error| {
                SandboxError::io(format!("{context}: creating {}", target.display()), error)
            })?;
    }
    let bind_flags = if source.is_dir() {
        (libc::MS_BIND | libc::MS_REC) as libc::c_ulong
    } else {
        libc::MS_BIND as libc::c_ulong
    };
    mount_path(Some(source), target, None, bind_flags, None, context)?;
    if read_only {
        let remount_flags = if source.is_dir() {
            (libc::MS_BIND | libc::MS_REMOUNT | libc::MS_RDONLY | libc::MS_REC) as libc::c_ulong
        } else {
            (libc::MS_BIND | libc::MS_REMOUNT | libc::MS_RDONLY) as libc::c_ulong
        };
        mount_path(None, target, None, remount_flags, None, context)?;
    }
    Ok(())
}

fn enter_pid_namespace() -> Result<()> {
    syscall_ok(
        unsafe { libc::unshare(libc::CLONE_NEWPID) },
        "unsharing pid namespace",
    )?;
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        return Err(SandboxError::io(
            "forking secure runner pid namespace init",
            std::io::Error::last_os_error(),
        ));
    }
    if pid > 0 {
        let mut status = 0;
        loop {
            let waited = unsafe { libc::waitpid(pid, &mut status, 0) };
            if waited == pid {
                break;
            }
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(SandboxError::io("waiting for pid namespace child", error));
        }
        let code = if libc::WIFEXITED(status) {
            libc::WEXITSTATUS(status)
        } else if libc::WIFSIGNALED(status) {
            128 + libc::WTERMSIG(status)
        } else {
            1
        };
        std::process::exit(code);
    }
    syscall_ok(unsafe { libc::setsid() }, "creating secure runner session")?;
    Ok(())
}

fn enter_chroot(root_dir: &Path) -> Result<()> {
    let root = path_cstring(root_dir, "secure runner root")?;
    syscall_ok(
        unsafe { libc::chdir(root.as_ptr()) },
        "changing directory to secure runner root",
    )?;
    syscall_ok(
        unsafe { libc::chroot(root.as_ptr()) },
        "chrooting secure runner into runtime root",
    )?;
    let slash = CString::new("/").expect("static slash");
    syscall_ok(
        unsafe { libc::chdir(slash.as_ptr()) },
        "changing directory to secure runner chroot root",
    )
}

fn mount_proc() -> Result<()> {
    mount(
        Some("proc"),
        "/proc",
        Some("proc"),
        (libc::MS_RDONLY | libc::MS_NOSUID | libc::MS_NODEV | libc::MS_NOEXEC) as libc::c_ulong,
        None,
        "mounting secure runner /proc",
    )
}

fn mount_private_tmp() -> Result<()> {
    mount(
        Some("tmpfs"),
        "/tmp",
        Some("tmpfs"),
        (libc::MS_NOSUID | libc::MS_NODEV | libc::MS_NOEXEC) as libc::c_ulong,
        Some("mode=700,size=16777216"),
        "mounting secure runner private /tmp",
    )
}

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
