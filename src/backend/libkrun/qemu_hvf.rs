use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::config::LibkrunRunnerConfig;
use crate::{Result, SandboxError};

const RPC_PORT_NAME: &str = "sagens-rpc";

pub(super) fn run(config: LibkrunRunnerConfig) -> Result<()> {
    let qemu = resolve_qemu_binary(&config)?;
    let kernel = resolve_qemu_kernel(&config)?;
    let bundle_dir = config
        .library_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));

    let mut command = Command::new(&qemu);
    command
        .arg("-accel")
        .arg("hvf")
        .arg("-machine")
        .arg("q35")
        .arg("-nodefaults")
        .arg("-cpu")
        .arg("host")
        .arg("-smp")
        .arg(config.cpu_cores.to_string())
        .arg("-m")
        .arg(config.memory_mb.to_string())
        .arg("-display")
        .arg("none")
        .arg("-monitor")
        .arg("none")
        .arg("-serial")
        .arg("none")
        .arg("-parallel")
        .arg("none")
        .arg("-kernel")
        .arg(&kernel)
        .arg("-append")
        .arg(config.kernel_cmdline())
        .arg("-device")
        .arg("virtio-serial-pci")
        .arg("-chardev")
        .arg(format!(
            "file,id=console,path={}",
            config.console_output_path.display()
        ))
        .arg("-device")
        .arg("virtconsole,chardev=console")
        .arg("-drive")
        .arg(format!(
            "if=none,id=rootfs,format=raw,file={},readonly=on",
            config.rootfs_image.display()
        ))
        .arg("-device")
        .arg("virtio-blk-pci,drive=rootfs")
        .arg("-drive")
        .arg(format!(
            "if=none,id=workspace,format=raw,file={}",
            config.workspace_image.display()
        ))
        .arg("-device")
        .arg("virtio-blk-pci,drive=workspace")
        .arg("-chardev")
        .arg(format!(
            "socket,id=rpc,path={},server=on,wait=off",
            config.vsock_socket.display()
        ))
        .arg("-device")
        .arg(format!("virtserialport,chardev=rpc,name={RPC_PORT_NAME}"));

    if config.network_enabled {
        command
            .arg("-netdev")
            .arg("user,id=net0")
            .arg("-device")
            .arg("virtio-net-pci,netdev=net0");
    }

    command.env("DYLD_LIBRARY_PATH", &bundle_dir);
    command.env("DYLD_FALLBACK_LIBRARY_PATH", &bundle_dir);

    let error = command.exec();
    Err(SandboxError::io(
        format!("execing {}", qemu.display()),
        error,
    ))
}

fn resolve_qemu_binary(config: &LibkrunRunnerConfig) -> Result<PathBuf> {
    let bundled = config
        .library_path
        .parent()
        .map(|dir| dir.join("qemu-system-x86_64"));
    if let Some(path) = bundled.filter(|path| path.is_file()) {
        return Ok(path);
    }
    if let Some(path) = std::env::var_os("PATH").and_then(|value| {
        std::env::split_paths(&value)
            .map(|dir| dir.join("qemu-system-x86_64"))
            .find(|candidate| candidate.is_file())
    }) {
        return Ok(path);
    }
    Err(SandboxError::backend(
        "missing bundled qemu-system-x86_64 runtime support",
    ))
}

fn resolve_qemu_kernel(config: &LibkrunRunnerConfig) -> Result<PathBuf> {
    if config.kernel_image.is_file() {
        return Ok(config.kernel_image.clone());
    }
    let file_name = config
        .kernel_image
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or_default();
    if let Some(raw_name) = file_name.strip_suffix(".pe.gz") {
        let candidate = config.kernel_image.with_file_name(raw_name);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(SandboxError::backend(format!(
        "missing raw kernel image for macos-x86_64 HVF backend: {}",
        config.kernel_image.display()
    )))
}
