pub const BACKEND_NAME: &str = "libkrun";

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub use upstream_libkrun::{
    krun_add_disk3, krun_add_vsock, krun_add_vsock_port2, krun_create_ctx,
    krun_disable_implicit_vsock, krun_free_ctx, krun_get_shutdown_eventfd, krun_init_log,
    krun_set_console_output, krun_set_exec, krun_set_firmware, krun_set_kernel,
    krun_set_kernel_console, krun_set_root_disk_remount, krun_set_vm_config, krun_set_workdir,
    krun_start_enter,
};
