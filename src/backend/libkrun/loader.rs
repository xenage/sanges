use std::ffi::CString;
use std::os::fd::RawFd;
use std::path::{Path, PathBuf};
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
type KrunSetWorkdir = unsafe extern "C" fn(u32, *const i8) -> i32;
type KrunSetExec = unsafe extern "C" fn(u32, *const i8, *const *const i8, *const *const i8) -> i32;
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
    set_workdir: KrunSetWorkdir,
    set_exec: KrunSetExec,
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
            set_workdir: unsafe { load(&library, b"krun_set_workdir\0") }?,
            set_exec: unsafe { load(&library, b"krun_set_exec\0") }?,
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
        let root_device = CString::new("/dev/vda").expect("static");
        let ext4 = CString::new("ext4").expect("static");
        let mount_options = CString::new("ro").expect("static");
        let kernel_image = kernel_image_for_libkrun(config)?;
        let kernel = to_cstring(&kernel_image)?;
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
        if config.boots_via_krun_init() {
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
            unsafe { (self.set_workdir)(ctx, workdir.as_ptr().cast()) },
            "krun_set_workdir",
        )?;
        call(
            unsafe {
                (self.set_exec)(
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

fn kernel_image_for_libkrun(config: &LibkrunRunnerConfig) -> Result<PathBuf> {
    if cfg!(all(target_os = "linux", target_arch = "x86_64"))
        && config.kernel_format == GuestKernelFormat::Raw
    {
        return pad_kernel_file_for_mmap(
            &config.kernel_image,
            &config.runtime_dir,
            host_page_size()?,
        );
    }
    Ok(config.kernel_image.clone())
}

fn pad_kernel_file_for_mmap(
    kernel_image: &Path,
    runtime_dir: &Path,
    page_size: usize,
) -> Result<PathBuf> {
    let metadata = std::fs::metadata(kernel_image)
        .map_err(|error| SandboxError::io("reading kernel image metadata", error))?;
    let kernel_size = usize::try_from(metadata.len())
        .map_err(|_| SandboxError::invalid("kernel image is too large to map"))?;
    let aligned_size = kernel_size.div_ceil(page_size) * page_size;
    if aligned_size == kernel_size {
        return Ok(kernel_image.to_path_buf());
    }
    std::fs::create_dir_all(runtime_dir)
        .map_err(|error| SandboxError::io("creating libkrun runtime directory", error))?;
    let file_name = kernel_image
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("kernel.raw");
    let padded_path = runtime_dir.join(format!("{file_name}.page-aligned"));
    std::fs::copy(kernel_image, &padded_path)
        .map_err(|error| SandboxError::io("copying kernel image for libkrun mmap", error))?;
    let padded = std::fs::OpenOptions::new()
        .write(true)
        .open(&padded_path)
        .map_err(|error| SandboxError::io("opening padded kernel image", error))?;
    padded
        .set_len(aligned_size as u64)
        .map_err(|error| SandboxError::io("extending padded kernel image", error))?;
    Ok(padded_path)
}

fn host_page_size() -> Result<usize> {
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if page_size <= 0 {
        return Err(SandboxError::backend(
            "failed to determine host page size for libkrun kernel mmap",
        ));
    }
    Ok(page_size as usize)
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
