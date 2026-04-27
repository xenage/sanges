use std::ffi::CString;
use std::os::fd::RawFd;
use std::path::Path;
use std::sync::OnceLock;

use sagens_libkrun as krun;

use super::config::LibkrunRunnerConfig;
use crate::{Result, SandboxError};

#[path = "loader/kernel.rs"]
mod kernel;
#[cfg(test)]
#[path = "loader/tests.rs"]
mod tests;
use kernel::{kernel_format, kernel_image_for_libkrun};

const KRUN_DISK_FORMAT_RAW: u32 = 0;
const KRUN_SYNC_RELAXED: u32 = 1;
const KRUN_LOG_LEVEL_TRACE: u32 = 5;
const KRUN_LOG_STYLE_NEVER: u32 = 2;
type KrunFn0 = unsafe extern "C" fn() -> i32;
type KrunInitLog = unsafe extern "C" fn(i32, u32, u32, u32) -> i32;

pub struct Libkrun;

pub struct PreparedMicrovm {
    ctx: Option<u32>,
    shutdown_fd: Option<RawFd>,
}

static KRUN_LOG_INIT: OnceLock<std::result::Result<(), String>> = OnceLock::new();

impl Libkrun {
    pub const fn new() -> Self {
        Self
    }

    pub unsafe fn prepare_microvm(self, config: &LibkrunRunnerConfig) -> Result<PreparedMicrovm> {
        init_log_once(&KRUN_LOG_INIT, krun::krun_init_log)?;
        let ctx = call_ctx(krun::krun_create_ctx, "krun_create_ctx")?;
        let rootfs = to_cstring(&config.rootfs_image)?;
        let workspace = to_cstring(&config.workspace_image)?;
        let vsock_socket = to_cstring(&config.vsock_socket)?;
        let console_output = to_cstring(&config.console_output_path)?;
        let hvc0 = CString::new("hvc0").expect("static");
        call(
            krun::krun_set_vm_config(ctx, config.cpu_cores as u8, config.memory_mb),
            "krun_set_vm_config",
        )?;
        call(
            unsafe { krun::krun_set_console_output(ctx, console_output.as_ptr().cast()) },
            "krun_set_console_output",
        )?;
        call(
            unsafe { krun::krun_set_kernel_console(ctx, hvc0.as_ptr().cast()) },
            "krun_set_kernel_console",
        )?;
        call(
            krun::krun_disable_implicit_vsock(ctx),
            "krun_disable_implicit_vsock",
        )?;
        unsafe { self.configure_direct_kernel_guest(ctx, config, &rootfs, &workspace) }?;
        call(krun::krun_add_vsock(ctx, 0), "krun_add_vsock")?;
        call(
            unsafe {
                krun::krun_add_vsock_port2(
                    ctx,
                    config.guest_vsock_port,
                    vsock_socket.as_ptr().cast(),
                    true,
                )
            },
            "krun_add_vsock_port2",
        )?;
        let shutdown_fd = call_optional_fd(
            krun::krun_get_shutdown_eventfd(ctx),
            "krun_get_shutdown_eventfd",
        )?;
        Ok(PreparedMicrovm {
            ctx: Some(ctx),
            shutdown_fd,
        })
    }

    unsafe fn configure_direct_kernel_guest(
        &self,
        ctx: u32,
        config: &LibkrunRunnerConfig,
        rootfs: &CString,
        workspace: &CString,
    ) -> Result<()> {
        let rootfs_id = CString::new("rootfs").expect("static");
        let workspace_id = CString::new("workspace").expect("static");
        let root_device = CString::new(config.root_device()).expect("static");
        let ext4 = CString::new("ext4").expect("static");
        let mount_options = CString::new("ro").expect("static");
        if let Some(firmware) = &config.firmware {
            let firmware = to_cstring(firmware)?;
            call(
                unsafe { krun::krun_set_firmware(ctx, firmware.as_ptr().cast()) },
                "krun_set_firmware",
            )?;
        }
        if !config.uses_krun_init() || linux_x86_64_raw_kernel_file(config) {
            let kernel_image = kernel_image_for_libkrun(config)?;
            let kernel = to_cstring(&kernel_image)?;
            let cmdline = CString::new(config.kernel_cmdline())
                .map_err(|_| SandboxError::invalid("kernel command line contains NUL"))?;
            call(
                unsafe {
                    krun::krun_set_kernel(
                        ctx,
                        kernel.as_ptr().cast(),
                        kernel_format(config.kernel_format),
                        std::ptr::null(),
                        cmdline.as_ptr().cast(),
                    )
                },
                "krun_set_kernel",
            )?;
        }
        call(
            unsafe {
                krun::krun_add_disk3(
                    ctx,
                    rootfs_id.as_ptr().cast(),
                    rootfs.as_ptr().cast(),
                    KRUN_DISK_FORMAT_RAW,
                    true,
                    false,
                    KRUN_SYNC_RELAXED,
                )
            },
            "krun_add_disk3(rootfs)",
        )?;
        call(
            unsafe {
                krun::krun_add_disk3(
                    ctx,
                    workspace_id.as_ptr().cast(),
                    workspace.as_ptr().cast(),
                    KRUN_DISK_FORMAT_RAW,
                    false,
                    false,
                    KRUN_SYNC_RELAXED,
                )
            },
            "krun_add_disk3(workspace)",
        )?;
        call(
            unsafe {
                krun::krun_set_root_disk_remount(
                    ctx,
                    root_device.as_ptr().cast(),
                    ext4.as_ptr().cast(),
                    mount_options.as_ptr().cast(),
                )
            },
            "krun_set_root_disk_remount",
        )?;
        if config.uses_krun_init() {
            unsafe { self.configure_krun_init_entrypoint(ctx, config) }?;
        }
        Ok(())
    }

    unsafe fn configure_krun_init_entrypoint(
        &self,
        ctx: u32,
        config: &LibkrunRunnerConfig,
    ) -> Result<()> {
        let workdir = CString::new("/").expect("static");
        let guest_agent = to_cstring(&config.guest_agent_path)?;
        let argv = [guest_agent.as_ptr().cast(), std::ptr::null()];
        let envp = [std::ptr::null()];
        call(
            unsafe { krun::krun_set_workdir(ctx, workdir.as_ptr().cast()) },
            "krun_set_workdir",
        )?;
        call(
            unsafe {
                krun::krun_set_exec(
                    ctx,
                    guest_agent.as_ptr().cast(),
                    argv.as_ptr(),
                    envp.as_ptr(),
                )
            },
            "krun_set_exec",
        )?;
        Ok(())
    }
}

impl PreparedMicrovm {
    pub fn shutdown_fd(&self) -> Option<RawFd> {
        self.shutdown_fd
    }

    pub fn start_enter(mut self) -> Result<()> {
        let ctx = self.ctx.take().expect("prepared microvm context");
        let rc = krun::krun_start_enter(ctx);
        let _ = krun::krun_free_ctx(ctx);
        if rc < 0 {
            return Err(SandboxError::backend(format!(
                "krun_start_enter failed with {rc}"
            )));
        }
        Ok(())
    }
}

fn linux_x86_64_raw_kernel_file(config: &LibkrunRunnerConfig) -> bool {
    cfg!(all(target_os = "linux", target_arch = "x86_64"))
        && config.kernel_format == crate::config::GuestKernelFormat::Raw
}

impl Drop for PreparedMicrovm {
    fn drop(&mut self) {
        if let Some(ctx) = self.ctx.take() {
            let _ = krun::krun_free_ctx(ctx);
        }
    }
}

fn call(code: i32, name: &str) -> Result<()> {
    if code < 0 {
        return Err(SandboxError::backend(format!("{name} failed with {code}")));
    }
    Ok(())
}

fn init_log_once(
    state: &OnceLock<std::result::Result<(), String>>,
    init_log: KrunInitLog,
) -> Result<()> {
    match state.get_or_init(|| {
        let code = unsafe { init_log(2, KRUN_LOG_LEVEL_TRACE, KRUN_LOG_STYLE_NEVER, 1) };
        if code < 0 {
            Err(format!("krun_init_log failed with {code}"))
        } else {
            Ok(())
        }
    }) {
        Ok(()) => Ok(()),
        Err(message) => Err(SandboxError::backend(message.clone())),
    }
}

fn call_ctx(create: KrunFn0, name: &str) -> Result<u32> {
    let code = unsafe { create() };
    if code < 0 {
        return Err(SandboxError::backend(format!("{name} failed with {code}")));
    }
    Ok(code as u32)
}

fn call_optional_fd(code: i32, name: &str) -> Result<Option<RawFd>> {
    if code == -libc::EINVAL {
        return Ok(None);
    }
    if code < 0 {
        return Err(SandboxError::backend(format!("{name} failed with {code}")));
    }
    Ok(Some(code))
}

fn to_cstring(path: &Path) -> Result<CString> {
    CString::new(path.as_os_str().to_string_lossy().as_bytes())
        .map_err(|_| SandboxError::invalid(format!("path contains NUL: {}", path.display())))
}
