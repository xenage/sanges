mod support;

use std::sync::Arc;

use sagens_host::boxes::{BoxManager, BoxStatus, LocalBoxService};
use sagens_host::config::{SandboxPolicy, WorkspaceConfig};
use sagens_host::protocol::{ExecExit, ExecRequest};
use sagens_host::workspace::WorkspaceStore;
use sagens_host::{BoxSettingValue, SandboxError};
use support::MockSandboxService;
use tempfile::tempdir;

#[tokio::test]
async fn starts_on_demand_and_reuses_active_box_before_idle() {
    let temp = tempdir().expect("tempdir");
    let runtime = Arc::new(MockSandboxService::default());
    let service = LocalBoxService::new(
        temp.path(),
        WorkspaceConfig { disk_size_mib: 64 },
        SandboxPolicy::default(),
        sagens_host::config::IsolationMode::Compat,
        runtime.clone(),
    )
    .await
    .expect("service");

    let box_record = service.create_box().await.expect("create box");
    let first = service
        .exec(box_record.box_id, ExecRequest::shell("echo first"))
        .await
        .expect("first exec")
        .collect()
        .await;
    let first_sandbox = runtime
        .active_sandbox(&box_record.box_id.to_string())
        .await
        .expect("active sandbox");

    let second = service
        .exec(box_record.box_id, ExecRequest::shell("echo second"))
        .await
        .expect("second exec")
        .collect()
        .await;
    let second_sandbox = runtime
        .active_sandbox(&box_record.box_id.to_string())
        .await
        .expect("active sandbox");

    assert_eq!(runtime.create_count().await, 1);
    assert_eq!(first.exit_status, ExecExit::Success);
    assert_eq!(second.exit_status, ExecExit::Success);
    assert_eq!(first_sandbox, second_sandbox);
}

#[tokio::test]
async fn restarts_after_idle_shutdown_and_preserves_workspace_identity() {
    let temp = tempdir().expect("tempdir");
    let runtime = Arc::new(MockSandboxService::default());
    let service = LocalBoxService::new(
        temp.path(),
        WorkspaceConfig { disk_size_mib: 64 },
        SandboxPolicy::default(),
        sagens_host::config::IsolationMode::Compat,
        runtime.clone(),
    )
    .await
    .expect("service");

    let box_record = service.create_box().await.expect("create box");
    service
        .write_file(box_record.box_id, "/workspace/keep.txt", b"hello", true)
        .await
        .expect("write file");
    let first_sandbox = runtime
        .active_sandbox(&box_record.box_id.to_string())
        .await
        .expect("active sandbox");

    runtime
        .expire_workspace(&box_record.box_id.to_string())
        .await;

    let file = service
        .read_file(box_record.box_id, "/workspace/keep.txt", 1024)
        .await
        .expect("read file after restart");
    let second_sandbox = runtime
        .active_sandbox(&box_record.box_id.to_string())
        .await
        .expect("restarted sandbox");
    let record = service
        .list_boxes()
        .await
        .expect("list boxes")
        .into_iter()
        .find(|record| record.box_id == box_record.box_id)
        .expect("box record");

    assert_eq!(runtime.create_count().await, 2);
    assert_ne!(first_sandbox, second_sandbox);
    assert_eq!(file.data, b"hello");
    assert_eq!(record.status, BoxStatus::Running);
    assert_eq!(record.workspace_path, box_record.workspace_path);
    let usage = record.runtime_usage.expect("runtime usage");
    assert_eq!(usage.cpu_millicores, 125);
    assert_eq!(usage.memory_used_mib, 64);
    assert_eq!(usage.fs_used_mib, 32);
    assert_eq!(usage.process_count, 2);
}

#[tokio::test]
async fn start_box_returns_live_runtime_usage() {
    let temp = tempdir().expect("tempdir");
    let runtime = Arc::new(MockSandboxService::default());
    let service = LocalBoxService::new(
        temp.path(),
        WorkspaceConfig { disk_size_mib: 64 },
        SandboxPolicy::default(),
        sagens_host::config::IsolationMode::Compat,
        runtime,
    )
    .await
    .expect("service");

    let record = service.create_box().await.expect("create box");
    let started = service.start_box(record.box_id).await.expect("start");
    let usage = started.runtime_usage.expect("runtime usage");

    assert_eq!(started.status, BoxStatus::Running);
    assert_eq!(usage.cpu_millicores, 125);
    assert_eq!(usage.memory_used_mib, 64);
    assert_eq!(usage.fs_used_mib, 32);
    assert_eq!(usage.process_count, 1);
}

#[tokio::test]
async fn list_boxes_marks_stale_running_boxes_stopped() {
    let temp = tempdir().expect("tempdir");
    let runtime = Arc::new(MockSandboxService::default());
    let service = LocalBoxService::new(
        temp.path(),
        WorkspaceConfig { disk_size_mib: 64 },
        SandboxPolicy::default(),
        sagens_host::config::IsolationMode::Compat,
        runtime.clone(),
    )
    .await
    .expect("service");

    let record = service.create_box().await.expect("create box");
    let started = service.start_box(record.box_id).await.expect("start");
    runtime.expire_workspace(&record.box_id.to_string()).await;

    let listed = service
        .list_boxes()
        .await
        .expect("list boxes")
        .into_iter()
        .find(|candidate| candidate.box_id == record.box_id)
        .expect("box record");

    assert_eq!(started.status, BoxStatus::Running);
    assert_eq!(listed.status, BoxStatus::Stopped);
    assert_eq!(listed.runtime_usage, None);
    assert_eq!(listed.active_sandbox_id, None);
    assert!(listed.last_stop_at_ms.is_some());
}

#[tokio::test]
async fn recycle_run_renews_sandbox_identity_without_dropping_prepared_state() {
    let temp = tempdir().expect("tempdir");
    let store = WorkspaceStore::new(temp.path(), WorkspaceConfig { disk_size_mib: 64 });
    store.ensure_layout().await.expect("layout");

    let run = store.prepare_run().await.expect("prepare run");
    let marker = run.root_dir.join("warm.marker");
    tokio::fs::write(&marker, b"warm").await.expect("marker");
    let old_sandbox_id = run.sandbox_id;
    assert_eq!(
        run.vsock_socket,
        std::path::PathBuf::from(format!("/tmp/asb-{}.sock", old_sandbox_id.simple()))
    );

    let recycled = store.recycle_run(run).await.expect("recycle run");

    assert_ne!(recycled.sandbox_id, old_sandbox_id);
    assert_eq!(
        recycled.vsock_socket,
        std::path::PathBuf::from(format!("/tmp/asb-{}.sock", recycled.sandbox_id.simple()))
    );
    assert_eq!(
        tokio::fs::read(recycled.root_dir.join("warm.marker"))
            .await
            .expect("marker contents"),
        b"warm"
    );
    assert!(
        !tokio::fs::try_exists(temp.path().join("runs").join(old_sandbox_id.to_string()))
            .await
            .expect("old run path")
    );
}

#[tokio::test]
async fn updates_persisted_box_settings_and_resizes_workspace_disk() {
    let temp = tempdir().expect("tempdir");
    let runtime = Arc::new(MockSandboxService::default());
    let service = LocalBoxService::new(
        temp.path(),
        WorkspaceConfig { disk_size_mib: 64 },
        SandboxPolicy::default(),
        sagens_host::config::IsolationMode::Compat,
        runtime,
    )
    .await
    .expect("service");

    let record = service.create_box().await.expect("create box");
    let updated = service
        .set_box_setting(record.box_id, BoxSettingValue::CpuCores { value: 2 })
        .await
        .expect("update cpu");
    assert_eq!(
        updated
            .settings
            .as_ref()
            .expect("settings")
            .cpu_cores
            .current,
        2
    );

    if has_ext4_resize_tools() {
        let updated = service
            .set_box_setting(record.box_id, BoxSettingValue::FsSizeMib { value: 96 })
            .await
            .expect("update fs size");
        assert_eq!(
            updated
                .settings
                .as_ref()
                .expect("settings")
                .fs_size_mib
                .current,
            96
        );
        assert_eq!(
            tokio::fs::metadata(&updated.workspace_path)
                .await
                .expect("workspace metadata")
                .len(),
            96 * 1024 * 1024
        );
    }
}

#[tokio::test]
async fn rejects_box_settings_update_while_box_is_running() {
    let temp = tempdir().expect("tempdir");
    let runtime = Arc::new(MockSandboxService::default());
    let service = LocalBoxService::new(
        temp.path(),
        WorkspaceConfig { disk_size_mib: 64 },
        SandboxPolicy::default(),
        sagens_host::config::IsolationMode::Compat,
        runtime,
    )
    .await
    .expect("service");

    let record = service.create_box().await.expect("create box");
    service.start_box(record.box_id).await.expect("start");
    let error = service
        .set_box_setting(record.box_id, BoxSettingValue::MemoryMb { value: 768 })
        .await
        .expect_err("set should fail");

    assert!(matches!(error, SandboxError::Conflict(_)));
}

fn has_ext4_resize_tools() -> bool {
    has_tool("resize2fs") && has_tool("e2fsck")
        || std::path::Path::new("/opt/homebrew/opt/e2fsprogs/sbin/resize2fs").exists()
            && std::path::Path::new("/opt/homebrew/opt/e2fsprogs/sbin/e2fsck").exists()
}

fn has_tool(tool: &str) -> bool {
    std::process::Command::new(tool)
        .arg("-V")
        .output()
        .map(|output| {
            output.status.success() || !output.stdout.is_empty() || !output.stderr.is_empty()
        })
        .unwrap_or(false)
}
