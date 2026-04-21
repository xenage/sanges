use std::ffi::CString;
use std::os::fd::RawFd;
use std::path::Path;
use std::sync::OnceLock;

use libloading::{Library, Symbol};

use super::config::LibkrunRunnerConfig;
use crate::config::GuestKernelFormat;
use crate::{Result, SandboxError};

#[cfg(test)]
#[path = "loader/tests.rs"]
mod tests;

const KRUN_DISK_FORMAT_RAW: u32 = 0;
const KRUN_SYNC_RELAXED: u32 = 1;
const KRUN_LOG_LEVEL_TRACE: u32 = 5;
const KRUN_LOG_STYLE_NEVER: u32 = 2;
const KRUN_KERNEL_FORMAT_RAW: u32 = 0;
const KRUN_KERNEL_FORMAT_ELF: u32 = 1;
const KRUN_KERNEL_FORMAT_PE_GZ: u32 = 2;
const KRUN_KERNEL_FORMAT_IMAGE_BZ2: u32 = 3;
const KRUN_KERNEL_FORMAT_IMAGE_GZ: u32 = 4;
const KRUN_KERNEL_FORMAT_IMAGE_ZSTD: u32 = 5;

type KrunFn0 = unsafe extern "C" fn() -> i32;
type KrunFreeCtx = unsafe extern "C" fn(u32) -> i32;
type KrunInitLog = unsafe extern "C" fn(i32, u32, u32, u32) -> i32;
type KrunVmConfig = unsafe extern "C" fn(u32, u8, u32) -> i32;
type KrunSetKernel = unsafe extern "C" fn(u32, *const i8, u32, *const i8, *const i8) -> i32;
type KrunSetFirmware = unsafe extern "C" fn(u32, *const i8) -> i32;
type KrunSetConsoleOutput = unsafe extern "C" fn(u32, *const i8) -> i32;
type KrunSetKernelConsole = unsafe extern "C" fn(u32, *const i8) -> i32;
type KrunAddDisk3 = unsafe extern "C" fn(u32, *const i8, *const i8, u32, bool, bool, u32) -> i32;
type KrunSetRootDiskRemount = unsafe extern "C" fn(u32, *const i8, *const i8, *const i8) -> i32;
type KrunFn1 = unsafe extern "C" fn(u32) -> i32;
type KrunAddVsock = unsafe extern "C" fn(u32, u32) -> i32;
type KrunAddVsockPort2 = unsafe extern "C" fn(u32, u32, *const i8, bool) -> i32;
type KrunGetShutdownEventfd = unsafe extern "C" fn(u32) -> i32;

pub struct Libkrun {
    _library: Library,
    create_ctx: KrunFn0,
    free_ctx: KrunFreeCtx,
    init_log: KrunInitLog,
    set_vm_config: KrunVmConfig,
    set_kernel: KrunSetKernel,
    set_firmware: KrunSetFirmware,
    set_console_output: KrunSetConsoleOutput,
    set_kernel_console: KrunSetKernelConsole,
    add_disk3: KrunAddDisk3,
    set_root_disk_remount: KrunSetRootDiskRemount,
    disable_implicit_vsock: KrunFn1,
    add_vsock: KrunAddVsock,
    add_vsock_port2: KrunAddVsockPort2,
    get_shutdown_eventfd: KrunGetShutdownEventfd,
    start_enter: KrunFn1,
}

pub struct PreparedMicrovm {
    libkrun: Libkrun,
    ctx: Option<u32>,
    shutdown_fd: Option<RawFd>,
}

static KRUN_LOG_INIT: OnceLock<std::result::Result<(), String>> = OnceLock::new();

impl Libkrun {
    pub fn load(path: &Path) -> Result<Self> {
        let library = unsafe { Library::new(path) }
            .map_err(|error| SandboxError::backend(format!("loading libkrun: {error}")))?;
        Ok(Self {
            create_ctx: unsafe { load(&library, b"krun_create_ctx\0") }?,
            free_ctx: unsafe { load(&library, b"krun_free_ctx\0") }?,
            init_log: unsafe { load(&library, b"krun_init_log\0") }?,
            set_vm_config: unsafe { load(&library, b"krun_set_vm_config\0") }?,
            set_kernel: unsafe { load(&library, b"krun_set_kernel\0") }?,
            set_firmware: unsafe { load(&library, b"krun_set_firmware\0") }?,
            set_console_output: unsafe { load(&library, b"krun_set_console_output\0") }?,
            set_kernel_console: unsafe { load(&library, b"krun_set_kernel_console\0") }?,
            add_disk3: unsafe { load(&library, b"krun_add_disk3\0") }?,
            set_root_disk_remount: unsafe { load(&library, b"krun_set_root_disk_remount\0") }?,
            disable_implicit_vsock: unsafe { load(&library, b"krun_disable_implicit_vsock\0") }?,
            add_vsock: unsafe { load(&library, b"krun_add_vsock\0") }?,
            add_vsock_port2: unsafe { load(&library, b"krun_add_vsock_port2\0") }?,
            get_shutdown_eventfd: unsafe { load(&library, b"krun_get_shutdown_eventfd\0") }?,
            start_enter: unsafe { load(&library, b"krun_start_enter\0") }?,
            _library: library,
        })
    }

    pub unsafe fn prepare_microvm(self, config: &LibkrunRunnerConfig) -> Result<PreparedMicrovm> {
        init_log_once(&KRUN_LOG_INIT, self.init_log)?;
        let ctx = call_ctx(self.create_ctx, "krun_create_ctx")?;
        let rootfs = to_cstring(&config.rootfs_image)?;
        let workspace = to_cstring(&config.workspace_image)?;
        let vsock_socket = to_cstring(&config.vsock_socket)?;
        let console_output = to_cstring(&config.console_output_path)?;
        let hvc0 = CString::new("hvc0").expect("static");
        call(
            unsafe { (self.set_vm_config)(ctx, config.cpu_cores as u8, config.memory_mb) },
            "krun_set_vm_config",
        )?;
        call(
            unsafe { (self.set_console_output)(ctx, console_output.as_ptr().cast()) },
            "krun_set_console_output",
        )?;
        call(
            unsafe { (self.set_kernel_console)(ctx, hvc0.as_ptr().cast()) },
            "krun_set_kernel_console",
        )?;
        call(
            unsafe { (self.disable_implicit_vsock)(ctx) },
            "krun_disable_implicit_vsock",
        )?;
        unsafe { self.configure_direct_kernel_guest(ctx, config, &rootfs, &workspace) }?;
        call(unsafe { (self.add_vsock)(ctx, 0) }, "krun_add_vsock")?;
        call(
            unsafe {
                (self.add_vsock_port2)(
                    ctx,
                    config.guest_vsock_port,
                    vsock_socket.as_ptr().cast(),
                    true,
                )
            },
            "krun_add_vsock_port2",
        )?;
        let shutdown_fd = call_optional_fd(
            unsafe { (self.get_shutdown_eventfd)(ctx) },
            "krun_get_shutdown_eventfd",
        )?;
        Ok(PreparedMicrovm {
            libkrun: self,
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
        let root_device = CString::new("/dev/vda1").expect("static");
        let ext4 = CString::new("ext4").expect("static");
        let mount_options = CString::new("ro").expect("static");
        let kernel = to_cstring(&config.kernel_image)?;
        let cmdline = CString::new(config.kernel_cmdline())
            .map_err(|_| SandboxError::invalid("kernel command line contains NUL"))?;
        if let Some(firmware) = &config.firmware {
            let firmware = to_cstring(firmware)?;
            call(
                unsafe { (self.set_firmware)(ctx, firmware.as_ptr().cast()) },
                "krun_set_firmware",
            )?;
        }
        call(
            unsafe {
                (self.set_kernel)(
                    ctx,
                    kernel.as_ptr().cast(),
                    kernel_format(config.kernel_format),
                    std::ptr::null(),
                    cmdline.as_ptr().cast(),
                )
            },
            "krun_set_kernel",
        )?;
        call(
            unsafe {
                (self.add_disk3)(
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
                (self.add_disk3)(
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
                (self.set_root_disk_remount)(
                    ctx,
                    root_device.as_ptr().cast(),
                    ext4.as_ptr().cast(),
                    mount_options.as_ptr().cast(),
                )
            },
            "krun_set_root_disk_remount",
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
        let rc = unsafe { (self.libkrun.start_enter)(ctx) };
        let _ = unsafe { (self.libkrun.free_ctx)(ctx) };
        if rc < 0 {
            return Err(SandboxError::backend(format!(
                "krun_start_enter failed with {rc}"
            )));
        }
        Ok(())
    }
}

impl Drop for PreparedMicrovm {
    fn drop(&mut self) {
        if let Some(ctx) = self.ctx.take() {
            let _ = unsafe { (self.libkrun.free_ctx)(ctx) };
        }
    }
}

unsafe fn load<T: Copy>(library: &Library, symbol: &[u8]) -> Result<T> {
    let symbol: Symbol<'_, T> = unsafe { library.get(symbol) }.map_err(|error| {
        SandboxError::backend(format!(
            "resolving {}: {error}",
            String::from_utf8_lossy(symbol)
        ))
    })?;
    Ok(*symbol)
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

fn kernel_format(format: GuestKernelFormat) -> u32 {
    match format {
        GuestKernelFormat::Raw => KRUN_KERNEL_FORMAT_RAW,
        GuestKernelFormat::Elf => KRUN_KERNEL_FORMAT_ELF,
        GuestKernelFormat::PeGz => KRUN_KERNEL_FORMAT_PE_GZ,
        GuestKernelFormat::ImageBz2 => KRUN_KERNEL_FORMAT_IMAGE_BZ2,
        GuestKernelFormat::ImageGz => KRUN_KERNEL_FORMAT_IMAGE_GZ,
        GuestKernelFormat::ImageZstd => KRUN_KERNEL_FORMAT_IMAGE_ZSTD,
    }
}
