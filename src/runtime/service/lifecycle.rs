use std::time::{SystemTime, UNIX_EPOCH};

use uuid::Uuid;

use super::AgentSandboxService;
use crate::backend::BackendLaunchRequest;
use crate::config::{IsolationMode, SandboxSpec};
use crate::guest_rpc::GuestRpcClient;
use crate::host_log;
use crate::runtime::registry::ManagedSandbox;
use crate::runtime::{SandboxSessionRecord, SandboxSessionState, SandboxSessionSummary};
use crate::{Result, SandboxError};

impl AgentSandboxService {
    pub(super) async fn create_sandbox_inner(
        &self,
        spec: SandboxSpec,
    ) -> Result<SandboxSessionSummary> {
        if self.config.isolation_mode == IsolationMode::Secure && spec.policy.network_enabled {
            return Err(SandboxError::invalid(
                "networking is unsupported in secure isolation mode",
            ));
        }
        if self.registry.has_active_workspace(&spec.workspace_id).await {
            return Err(SandboxError::invalid(format!(
                "workspace {} already has an active sandbox",
                spec.workspace_id
            )));
        }
        if let Some(commit_id) = &spec.restore_commit {
            self.workspace
                .restore_internal_checkpoint(&spec.workspace_id, commit_id)
                .await?;
        }
        let workspace = self.workspace.prepare_workspace(&spec.workspace_id).await?;
        let run_layout = match self.registry.take_warm_run().await {
            Some(run_layout) => self.workspace.recycle_run(run_layout).await?,
            None => self.workspace.prepare_run().await?,
        };
        host_log::emit(
            "runtime",
            format!(
                "launching sandbox_id={} workspace_id={} run_root={} runner_log={} guest_console_log={}",
                run_layout.sandbox_id,
                workspace.workspace_id,
                run_layout.root_dir.display(),
                run_layout.runner_log.display(),
                run_layout.guest_console_log.display()
            ),
        );
        let launch = match self
            .backend
            .launch(BackendLaunchRequest {
                sandbox_id: run_layout.sandbox_id,
                run_layout: run_layout.clone(),
                guest: self.config.guest.clone(),
                policy: spec.policy,
                workspace: workspace.clone(),
                hardening: self.config.hardening.clone(),
                isolation_mode: self.config.isolation_mode,
                artifact_bundle: self.config.artifact_bundle.clone(),
            })
            .await
        {
            Ok(launch) => launch,
            Err(error) => {
                let error = enrich_launch_error(&run_layout, "backend launch", error);
                log_launch_failure(
                    &workspace.workspace_id,
                    &run_layout,
                    "backend launch",
                    &error,
                );
                return Err(error);
            }
        };
        let guest =
            match GuestRpcClient::connect(&launch.guest_endpoint, self.config.guest.boot_timeout)
                .await
            {
                Ok(guest) => guest,
                Err(error) => {
                    let error = enrich_launch_error(&run_layout, "guest connect", error);
                    log_launch_failure(
                        &workspace.workspace_id,
                        &run_layout,
                        "guest connect",
                        &error,
                    );
                    return Err(error);
                }
            };
        let summary = SandboxSessionSummary {
            sandbox_id: run_layout.sandbox_id,
            workspace_id: workspace.workspace_id.clone(),
            state: SandboxSessionState::Active,
            policy: spec.policy,
            started_at_ms: now_ms(),
            ended_at_ms: None,
        };
        host_log::emit(
            "runtime",
            format!(
                "sandbox ready sandbox_id={} workspace_id={}",
                summary.sandbox_id, summary.workspace_id
            ),
        );
        let baseline = guest.snapshot_workspace().await?;
        self.registry
            .insert(ManagedSandbox::new(
                summary.clone(),
                baseline,
                run_layout,
                workspace,
                launch.instance,
                guest,
            ))
            .await;
        Ok(summary)
    }

    pub(super) async fn destroy_sandbox_inner(
        &self,
        sandbox_id: Uuid,
    ) -> Result<SandboxSessionRecord> {
        if let Some(record) = self.registry.history_record(sandbox_id).await {
            return Ok(record);
        }
        let session = self.registry.remove_active(sandbox_id).await?;
        host_log::emit(
            "runtime",
            format!(
                "destroying sandbox_id={} workspace_id={} run_root={}",
                sandbox_id,
                session.workspace.workspace_id,
                session.run_layout.root_dir.display()
            ),
        );
        let _ = session.guest.sync_workspace().await;
        let final_snapshot = session.snapshot_for_diff().await?;
        let changes = session.baseline.diff(&final_snapshot);
        let mut summary = session.summary.write().await;
        summary.state = SandboxSessionState::Destroyed;
        summary.ended_at_ms = Some(now_ms());
        let record = SandboxSessionRecord {
            summary: summary.clone(),
            changes,
        };
        drop(summary);
        let _ = session.guest.sync_workspace().await;
        let _ = session.guest.shutdown().await;
        session.backend.shutdown().await?;
        self.registry
            .store_warm_run(
                session.run_layout.clone(),
                self.config.lifecycle.warm_pool_size,
            )
            .await;
        self.registry.save_history(record.clone()).await;
        host_log::emit(
            "runtime",
            format!(
                "destroyed sandbox_id={} workspace_id={}",
                sandbox_id, record.summary.workspace_id
            ),
        );
        Ok(record)
    }

    pub(super) async fn restore_workspace_checkpoint_inner(
        &self,
        workspace_id: &str,
        checkpoint_id: &str,
    ) -> Result<crate::workspace::WorkspaceCommitRecord> {
        if self.registry.has_active_workspace(workspace_id).await {
            return Err(SandboxError::invalid(format!(
                "workspace {} is currently attached to an active sandbox",
                workspace_id
            )));
        }
        self.workspace
            .restore_internal_checkpoint(workspace_id, checkpoint_id)
            .await
    }
}

fn now_ms() -> u64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_millis() as u64,
        Err(_) => 0,
    }
}

fn log_launch_failure(
    workspace_id: &str,
    run_layout: &crate::workspace::RunLayout,
    stage: &str,
    error: &crate::SandboxError,
) {
    host_log::emit(
        "runtime",
        format!(
            "{stage} failed sandbox_id={} workspace_id={} run_root={} error={error}",
            run_layout.sandbox_id,
            workspace_id,
            run_layout.root_dir.display()
        ),
    );
    host_log::emit_file_excerpt("runtime", "libkrun-runner", &run_layout.runner_log, 40);
    host_log::emit_file_excerpt(
        "runtime",
        "guest-console",
        &run_layout.guest_console_log,
        40,
    );
}

fn enrich_launch_error(
    run_layout: &crate::workspace::RunLayout,
    stage: &str,
    error: crate::SandboxError,
) -> crate::SandboxError {
    if !should_attach_boot_detail(stage, &error) {
        return error;
    }
    let Some(detail) = boot_failure_detail(run_layout) else {
        return error;
    };
    let rendered = error.to_string();
    if rendered.contains(&detail) {
        return error;
    }
    SandboxError::backend(format!("{stage} failed: {detail}"))
}

fn should_attach_boot_detail(stage: &str, error: &crate::SandboxError) -> bool {
    match (stage, error) {
        ("guest connect", SandboxError::Timeout(_)) => true,
        ("guest connect", SandboxError::Protocol(_)) => true,
        ("guest connect", SandboxError::Io { context, .. })
            if context.contains("guest vsock bridge") =>
        {
            true
        }
        ("backend launch", SandboxError::Timeout(_)) => true,
        ("backend launch", SandboxError::Backend(message))
            if message.contains("startup handshake")
                || message.contains("exited before")
                || message.contains("thread panicked") =>
        {
            true
        }
        _ => false,
    }
}

fn boot_failure_detail(run_layout: &crate::workspace::RunLayout) -> Option<String> {
    let guest_console = host_log::read_file_tail_lossy(&run_layout.guest_console_log, 200).ok();
    let runner_log = host_log::read_file_tail_lossy(&run_layout.runner_log, 200).ok();
    select_boot_failure_line(guest_console.as_deref(), &GUEST_FAILURE_PATTERNS)
        .or_else(|| select_boot_failure_line(runner_log.as_deref(), &RUNNER_FAILURE_PATTERNS))
        .or_else(|| last_nonempty_line(guest_console.as_deref()))
        .or_else(|| last_nonempty_line(runner_log.as_deref()))
}

const GUEST_FAILURE_PATTERNS: &[&str] = &[
    "Kernel panic - not syncing:",
    "VFS: Cannot open root device",
    "VFS: Unable to mount root fs",
    "Can't open blockdev",
    "No working init found",
    "Run /init as init process",
    "mount:",
    "EXT4-fs error",
    "failed",
    "error",
];

const RUNNER_FAILURE_PATTERNS: &[&str] = &[
    "unexpected exception:",
    "panicked at",
    "krun_start_enter failed",
    "failed with",
    "error",
];

fn select_boot_failure_line(text: Option<&str>, patterns: &[&str]) -> Option<String> {
    let text = text?;
    text.lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty() && patterns.iter().any(|pattern| line.contains(pattern)))
        .map(str::to_owned)
}

fn last_nonempty_line(text: Option<&str>) -> Option<String> {
    let text = text?;
    text.lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
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
            vsock_socket: root_dir.join("guest.sock"),
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
}
