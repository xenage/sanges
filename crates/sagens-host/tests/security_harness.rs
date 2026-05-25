mod support;

#[cfg(target_os = "linux")]
mod linux_secure_harness {
    use std::fs;
    use std::process::Command;

    use serde_json::json;
    use tempfile::tempdir;

    use super::support::e2e;

    #[test]
    fn secure_runner_harness_passes_when_explicitly_enabled() {
        if !enabled() {
            return;
        }

        let temp = tempdir().expect("tempdir");
        let run_root = temp.path().join("run");
        let runtime_dir = run_root.join("runtime");
        fs::create_dir_all(&runtime_dir).expect("runtime dir");

        let kernel = temp.path().join("kernel");
        let rootfs = temp.path().join("rootfs.raw");
        let workspace = temp.path().join("workspace.raw");
        fs::write(&kernel, b"kernel").expect("kernel");
        fs::write(&rootfs, b"rootfs").expect("rootfs");
        fs::write(&workspace, b"workspace").expect("workspace");

        let config_path = temp.path().join("runner.json");
        fs::write(
            &config_path,
            serde_json::to_vec_pretty(&json!({
                "kernel_image": kernel,
                "kernel_format": "ImageGz",
                "rootfs_image": rootfs,
                "workspace_image": workspace,
                "runtime_dir": runtime_dir,
                "console_output_path": run_root.join("guest-console.log"),
                "firmware": serde_json::Value::Null,
                "guest_agent_path": "/usr/local/bin/sagens-guest-agent",
                "cpu_cores": 1,
                "memory_mb": 128,
                "tmpfs_mib": 64,
                "max_processes": 32,
                "network_enabled": false,
                "guest_uid": 65534,
                "guest_gid": 65534,
                "guest_vsock_port": 11000,
                "vsock_socket": runtime_dir.join("guest.sock"),
                "isolation_mode": "secure",
                "runner_log_limit_bytes": 4 * 1024 * 1024u64,
            }))
            .expect("encode config"),
        )
        .expect("write config");

        let status = Command::new(e2e::host_binary())
            .arg("__security-harness")
            .arg(&config_path)
            .status()
            .expect("run security harness");
        assert!(status.success(), "security harness exited with {status}");
    }

    fn enabled() -> bool {
        matches!(
            std::env::var("SAGENS_RUN_SECURE_HARNESS").ok().as_deref(),
            Some("1" | "true" | "yes")
        )
    }
}
