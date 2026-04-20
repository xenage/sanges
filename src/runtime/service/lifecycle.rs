use std::time::{SystemTime, UNIX_EPOCH};

use uuid::Uuid;

use super::AgentSandboxService;
use crate::backend::BackendLaunchRequest;
use crate::config::{IsolationMode, SandboxSpec};
use crate::guest_rpc::GuestRpcClient;
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
        let launch = self
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
            .await?;
        let guest =
            GuestRpcClient::connect(&launch.guest_endpoint, self.config.guest.boot_timeout).await?;
        let summary = SandboxSessionSummary {
            sandbox_id: run_layout.sandbox_id,
            workspace_id: workspace.workspace_id.clone(),
            state: SandboxSessionState::Active,
            policy: spec.policy,
            started_at_ms: now_ms(),
            ended_at_ms: None,
        };
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
