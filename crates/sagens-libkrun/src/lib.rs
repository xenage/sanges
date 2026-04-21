pub const BACKEND_NAME: &str = "libkrun";

#[cfg(target_os = "linux")]
pub mod ffi {
    use krun_linked as linked;

    pub unsafe extern "C" fn create_ctx() -> i32 {
        unsafe { linked::krun_create_ctx() }
    }

    pub unsafe extern "C" fn free_ctx(ctx_id: u32) -> i32 {
        unsafe { linked::krun_free_ctx(ctx_id) }
    }

    pub unsafe extern "C" fn init_log(log_fd: i32, level: u32, style: u32, truncate: u32) -> i32 {
        unsafe { linked::krun_init_log(log_fd, level, style, truncate) }
    }

    pub unsafe extern "C" fn set_vm_config(ctx_id: u32, num_vcpus: u8, ram_mib: u32) -> i32 {
        unsafe { linked::krun_set_vm_config(ctx_id, num_vcpus, ram_mib) }
    }

    pub unsafe extern "C" fn set_kernel(
        ctx_id: u32,
        kernel_path: *const i8,
        kernel_format: u32,
        initramfs_path: *const i8,
        cmdline: *const i8,
    ) -> i32 {
        unsafe {
            linked::krun_set_kernel(
                ctx_id,
                kernel_path.cast(),
                kernel_format,
                initramfs_path.cast(),
                cmdline.cast(),
            )
        }
    }

    pub unsafe extern "C" fn set_firmware(ctx_id: u32, firmware_path: *const i8) -> i32 {
        unsafe { linked::krun_set_firmware(ctx_id, firmware_path.cast()) }
    }

    pub unsafe extern "C" fn set_console_output(ctx_id: u32, path: *const i8) -> i32 {
        unsafe { linked::krun_set_console_output(ctx_id, path.cast()) }
    }

    pub unsafe extern "C" fn set_kernel_console(ctx_id: u32, console_id: *const i8) -> i32 {
        unsafe { linked::krun_set_kernel_console(ctx_id, console_id.cast()) }
    }

    pub unsafe extern "C" fn add_disk3(
        ctx_id: u32,
        block_id: *const i8,
        disk_path: *const i8,
        disk_format: u32,
        read_only: bool,
        direct_io: bool,
        sync_mode: u32,
    ) -> i32 {
        unsafe {
            linked::krun_add_disk3(
                ctx_id,
                block_id.cast(),
                disk_path.cast(),
                disk_format,
                read_only,
                direct_io,
                sync_mode,
            )
        }
    }

    pub unsafe extern "C" fn set_root_disk_remount(
        ctx_id: u32,
        device: *const i8,
        fstype: *const i8,
        options: *const i8,
    ) -> i32 {
        unsafe {
            linked::krun_set_root_disk_remount(ctx_id, device.cast(), fstype.cast(), options.cast())
        }
    }

    pub unsafe extern "C" fn disable_implicit_vsock(ctx_id: u32) -> i32 {
        linked::krun_disable_implicit_vsock(ctx_id)
    }

    pub unsafe extern "C" fn add_vsock(ctx_id: u32, tsi_features: u32) -> i32 {
        linked::krun_add_vsock(ctx_id, tsi_features)
    }

    pub unsafe extern "C" fn add_vsock_port2(
        ctx_id: u32,
        port: u32,
        filepath: *const i8,
        listen: bool,
    ) -> i32 {
        unsafe { linked::krun_add_vsock_port2(ctx_id, port, filepath.cast(), listen) }
    }

    pub unsafe extern "C" fn get_shutdown_eventfd(ctx_id: u32) -> i32 {
        linked::krun_get_shutdown_eventfd(ctx_id)
    }

    pub unsafe extern "C" fn start_enter(ctx_id: u32) -> i32 {
        linked::krun_start_enter(ctx_id)
    }
}
