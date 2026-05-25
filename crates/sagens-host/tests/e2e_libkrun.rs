mod support;

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod host_e2e {
    use std::io::ErrorKind;
    use std::net::TcpListener;
    use std::path::Path;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::Duration;

    use sagens_host::auth::{AdminCredential, AdminStore, BoxCredentialStore, UserConfig};
    use sagens_host::boxes::{BoxManager, LocalBoxService};
    use sagens_host::config::{IsolationMode, LifecycleConfig, SandboxPolicy};
    use sagens_host::runtime::{AgentSandboxService, SandboxService};
    use sagens_host::{
        ArtifactBundle, ControlPlaneConfig, ExecExit, GuestConfig, GuestKernelFormat,
        HardeningConfig, RuntimeConfig, WorkspaceConfig, serve_box_api_websocket,
    };

    use super::support::e2e::{
        configure_libkrun_test_helper, default_wheelhouse, enabled, env, guest_assets, state_dir,
        upload_directory_box,
    };

    #[tokio::test]
    async fn libkrun_runs_unified_box_websocket_and_preserves_workspace_across_restart() {
        if !enabled() {
            return;
        }

        let wheelhouse = env("SAGENS_WHEELHOUSE")
            .map(PathBuf::from)
            .or_else(default_wheelhouse)
            .expect("local wheelhouse must exist in SAGENS_WHEELHOUSE or .e2e-wheelhouse");
        configure_libkrun_test_helper();
        let guest_assets = guest_assets();
        let guest_kernel_format = GuestKernelFormat::detect_from_path(
            &guest_assets.kernel_image,
            GuestKernelFormat::default_for_host(),
        );
        let state_dir = state_dir();
        let runtime_config = RuntimeConfig {
            state_dir: state_dir.clone(),
            guest: GuestConfig {
                kernel_image: guest_assets.kernel_image,
                kernel_format: guest_kernel_format,
                rootfs_image: guest_assets.rootfs_image,
                firmware: guest_assets.firmware,
                guest_agent_path: env("SAGENS_GUEST_AGENT_PATH")
                    .unwrap_or_else(|| "/usr/local/bin/sagens-guest-agent".into())
                    .into(),
                guest_vsock_port: 11_000,
                boot_timeout: Duration::from_secs(30),
                guest_uid: compat_guest_uid(),
                guest_gid: compat_guest_gid(),
                guest_tmpfs_mib: 64,
            },
            workspace: WorkspaceConfig { disk_size_mib: 128 },
            control: ControlPlaneConfig::default(),
            lifecycle: LifecycleConfig::default(),
            isolation_mode: IsolationMode::Compat,
            hardening: HardeningConfig {
                enable_landlock: false,
                cgroup_parent: None,
                runner_log_limit_bytes: 4 * 1024 * 1024,
            },
            artifact_bundle: ArtifactBundle {
                bundle_id: "e2e".into(),
            },
            default_policy: SandboxPolicy::default(),
        };
        let runtime: Arc<dyn SandboxService> = Arc::new(
            AgentSandboxService::new(runtime_config.clone())
                .await
                .expect("runtime"),
        );
        let service: Arc<dyn BoxManager> = Arc::new(
            LocalBoxService::new(
                state_dir.clone(),
                WorkspaceConfig { disk_size_mib: 128 },
                SandboxPolicy::default(),
                IsolationMode::Compat,
                runtime,
            )
            .await
            .expect("box service"),
        );
        let admin_store = Arc::new(AdminStore::new(&state_dir));
        let box_credential_store = Arc::new(BoxCredentialStore::new(&state_dir));
        let admin = AdminCredential {
            admin_uuid: uuid::Uuid::new_v4(),
            admin_token: "e2e-admin-token".into(),
        };
        admin_store.bootstrap(&admin).await.expect("bootstrap");
        let server = serve_box_api_websocket(
            "127.0.0.1:0".parse().expect("addr"),
            service,
            admin_store,
            box_credential_store,
            sagens_host::ImageApiConfig::from_runtime_config(&runtime_config)
                .await
                .expect("image api config"),
        )
        .await
        .expect("server");
        let client = sagens_host::BoxApiClient::connect(&UserConfig {
            version: 1,
            admin_uuid: admin.admin_uuid,
            admin_token: admin.admin_token,
            endpoint: format!("ws://{}", server.addr),
        })
        .await
        .expect("client");

        let box_id = client.create_box().await.expect("create").box_id;
        client.start_box(box_id).await.expect("start");

        let python = client
            .exec_python_capture(
                box_id,
                vec![
                    "-c".into(),
                    "from pathlib import Path; Path('box.txt').write_text('persisted'); print('python-e2e')".into(),
                ],
            )
            .await
            .expect("python");
        assert_eq!(python.exit_status, ExecExit::Success);
        upload_directory_box(
            &client,
            box_id,
            wheelhouse.as_path(),
            Path::new(".wheelhouse"),
        )
        .await;
        let pip = client
            .exec_bash_capture(
                box_id,
                "python3 -m pip install --no-index --find-links .wheelhouse --target .sandbox-pkgs colorama".into(),
            )
            .await
            .expect("pip");
        assert_eq!(pip.exit_status, ExecExit::Success);

        client.stop_box(box_id).await.expect("stop");
        client.start_box(box_id).await.expect("restart");

        let verify = client
            .exec_python_capture(
                box_id,
                vec![
                    "-c".into(),
                    "import sys; sys.path.insert(0, '.sandbox-pkgs'); import colorama; print(open('box.txt').read(), colorama.__name__)".into(),
                ],
            )
            .await
            .expect("verify");
        assert_eq!(verify.exit_status, ExecExit::Success);
        let stdout = String::from_utf8_lossy(&verify.stdout);
        assert!(stdout.contains("persisted"));
        assert!(stdout.contains("colorama"));

        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind host listener");
        listener
            .set_nonblocking(true)
            .expect("set nonblocking listener");
        let port = listener.local_addr().expect("listener addr").port();
        let network = client
            .exec_python_capture(
                box_id,
                vec![
                    "-c".into(),
                    r#"import socket, sys
port = int(sys.argv[1])
checks = [
    ("connect", lambda: socket.create_connection(("127.0.0.1", port), timeout=1)),
    ("bind", lambda: socket.socket(socket.AF_INET, socket.SOCK_STREAM).bind(("127.0.0.1", 0))),
]
for label, attempt in checks:
    try:
        sock = attempt()
    except OSError as exc:
        print(f"{label}:blocked:{exc.errno}")
    else:
        try:
            print(f"{label}:unexpected-success")
        finally:
            sock.close()
        raise SystemExit(90)
"#
                    .into(),
                    port.to_string(),
                ],
            )
            .await
            .expect("network probe");
        assert_eq!(network.exit_status, ExecExit::Success);
        let network_stdout = String::from_utf8_lossy(&network.stdout);
        assert!(network_stdout.contains("connect:blocked"));
        assert!(network_stdout.contains("bind:blocked"));
        let accept_error = listener
            .accept()
            .expect_err("guest network should not reach host");
        assert_eq!(accept_error.kind(), ErrorKind::WouldBlock);
    }

    fn compat_guest_uid() -> u32 {
        if cfg!(target_os = "macos") { 0 } else { 65_534 }
    }

    fn compat_guest_gid() -> u32 {
        if cfg!(target_os = "macos") { 0 } else { 65_534 }
    }
}
