use std::fs;
use std::path::PathBuf;

use tempfile::tempdir;
use uuid::Uuid;

use super::{boot_failure_detail, enrich_launch_error};
use crate::{SandboxError, workspace::RunLayout};

fn run_layout(root_dir: PathBuf) -> RunLayout {
    RunLayout {
        sandbox_id: Uuid::new_v4(),
        root_dir: root_dir.clone(),
        runtime_dir: root_dir.join("runtime"),
        runner_config: root_dir.join("runner-config.json"),
        runner_log: root_dir.join("libkrun-runner.log"),
        guest_console_log: root_dir.join("guest-console.log"),
        vsock_socket: root_dir.join("runtime").join("guest.sock"),
    }
}

#[test]
fn prefers_kernel_panic_from_guest_console() {
    let temp = tempdir().expect("tempdir");
    let run_layout = run_layout(temp.path().to_path_buf());
    fs::write(
        &run_layout.guest_console_log,
        "booting\nKernel panic - not syncing: VFS: Unable to mount root fs on unknown-block(254,0)\n",
    )
    .expect("write guest console");
    fs::write(
        &run_layout.runner_log,
        "thread 'fc_vcpu 0' panicked at unexpected exception: 0x20\n",
    )
    .expect("write runner log");

    let detail = boot_failure_detail(&run_layout).expect("detail");

    assert_eq!(
        detail,
        "Kernel panic - not syncing: VFS: Unable to mount root fs on unknown-block(254,0)"
    );
}

#[test]
fn falls_back_to_runner_log_when_guest_console_is_empty() {
    let temp = tempdir().expect("tempdir");
    let run_layout = run_layout(temp.path().to_path_buf());
    fs::write(&run_layout.guest_console_log, "\n").expect("write guest console");
    fs::write(
        &run_layout.runner_log,
        "thread 'fc_vcpu 0' panicked at src/main.rs: unexpected exception: 0x20\n",
    )
    .expect("write runner log");

    let detail = boot_failure_detail(&run_layout).expect("detail");

    assert_eq!(
        detail,
        "thread 'fc_vcpu 0' panicked at src/main.rs: unexpected exception: 0x20"
    );
}

#[test]
fn replaces_guest_connect_timeout_with_boot_detail() {
    let temp = tempdir().expect("tempdir");
    let run_layout = run_layout(temp.path().to_path_buf());
    fs::write(
        &run_layout.guest_console_log,
        "Kernel panic - not syncing: VFS: Unable to mount root fs on unknown-block(254,0)\n",
    )
    .expect("write guest console");

    let error = enrich_launch_error(
        &run_layout,
        "guest connect",
        SandboxError::timeout("timed out waiting for guest vsock bridge"),
    );

    assert_eq!(
        error.to_string(),
        "backend failure: guest connect failed: Kernel panic - not syncing: VFS: Unable to mount root fs on unknown-block(254,0)"
    );
}
