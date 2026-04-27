use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{Mutex, OwnedSemaphorePermit, RwLock, Semaphore};
use uuid::Uuid;

use crate::backend::BackendInstance;
use crate::guest_rpc::GuestRpcClient;
use crate::runtime::{SandboxSessionRecord, SandboxSessionSummary};
use crate::workspace::{RunLayout, WorkspaceLease, WorkspaceSnapshot};
use crate::{Result, SandboxError};

#[derive(Clone)]
pub(super) struct SessionRegistry {
    inner: Arc<RegistryState>,
}

struct RegistryState {
    active: RwLock<HashMap<Uuid, Arc<ManagedSandbox>>>,
    warm_runs: Mutex<Vec<RunLayout>>,
    history: RwLock<HashMap<Uuid, SandboxSessionRecord>>,
}

impl SessionRegistry {
    pub(super) fn new() -> Self {
        Self {
            inner: Arc::new(RegistryState {
                active: RwLock::new(HashMap::new()),
                warm_runs: Mutex::new(Vec::new()),
                history: RwLock::new(HashMap::new()),
            }),
        }
    }

    pub(super) async fn insert(&self, sandbox: ManagedSandbox) -> Arc<ManagedSandbox> {
        let sandbox = Arc::new(sandbox);
        self.inner
            .active
            .write()
            .await
            .insert(sandbox.summary.read().await.sandbox_id, sandbox.clone());
        sandbox
    }

    pub(super) async fn active(&self, sandbox_id: Uuid) -> Result<Arc<ManagedSandbox>> {
        self.inner
            .active
            .read()
            .await
            .get(&sandbox_id)
            .cloned()
            .ok_or_else(|| SandboxError::invalid(format!("unknown active sandbox {sandbox_id}")))
    }

    pub(super) async fn remove_active(&self, sandbox_id: Uuid) -> Result<Arc<ManagedSandbox>> {
        self.inner
            .active
            .write()
            .await
            .remove(&sandbox_id)
            .ok_or_else(|| SandboxError::invalid(format!("unknown active sandbox {sandbox_id}")))
    }

    pub(super) async fn has_active_workspace(&self, workspace_id: &str) -> bool {
        for sandbox in self.inner.active.read().await.values() {
            if sandbox.workspace.workspace_id == workspace_id {
                return true;
            }
        }
        false
    }

    pub(super) async fn list(&self, include_history: bool) -> Vec<SandboxSessionSummary> {
        let mut sessions = Vec::new();
        for sandbox in self.inner.active.read().await.values() {
            sessions.push(sandbox.summary.read().await.clone());
        }
        if include_history {
            sessions.extend(
                self.inner
                    .history
                    .read()
                    .await
                    .values()
                    .map(|record| record.summary.clone()),
            );
        }
        sessions.sort_by_key(|summary| (summary.started_at_ms, summary.sandbox_id));
        sessions
    }

    pub(super) async fn history_record(&self, sandbox_id: Uuid) -> Option<SandboxSessionRecord> {
        self.inner.history.read().await.get(&sandbox_id).cloned()
    }

    pub(super) async fn save_history(&self, record: SandboxSessionRecord) {
        self.inner
            .history
            .write()
            .await
            .insert(record.summary.sandbox_id, record);
    }

    pub(super) async fn take_warm_run(&self) -> Option<RunLayout> {
        self.inner.warm_runs.lock().await.pop()
    }

    pub(super) async fn store_warm_run(&self, run: RunLayout, warm_pool_size: usize) {
        let mut warm_runs = self.inner.warm_runs.lock().await;
        warm_runs.push(run);
        while warm_runs.len() > warm_pool_size {
            warm_runs.remove(0);
        }
    }
}

pub(super) struct ManagedSandbox {
    pub(super) summary: RwLock<SandboxSessionSummary>,
    pub(super) baseline: WorkspaceSnapshot,
    pub(super) run_layout: RunLayout,
    pub(super) workspace: WorkspaceLease,
    pub(super) backend: Arc<dyn BackendInstance>,
    pub(super) guest: GuestRpcClient,
    exec_gate: Arc<Semaphore>,
}

impl ManagedSandbox {
    pub(super) fn new(
        summary: SandboxSessionSummary,
        baseline: WorkspaceSnapshot,
        run_layout: RunLayout,
        workspace: WorkspaceLease,
        backend: Arc<dyn BackendInstance>,
        guest: GuestRpcClient,
    ) -> Self {
        Self {
            summary: RwLock::new(summary),
            baseline,
            run_layout,
            workspace,
            backend,
            guest,
            exec_gate: Arc::new(Semaphore::new(1)),
        }
    }

    pub(super) async fn snapshot_for_diff(&self) -> Result<WorkspaceSnapshot> {
        self.guest.snapshot_workspace().await
    }

    pub(super) fn try_acquire_exec(&self) -> Result<OwnedSemaphorePermit> {
        self.exec_gate.clone().try_acquire_owned().map_err(|_| {
            SandboxError::conflict("parallel exec is not supported for a running BOX".to_string())
        })
    }
}
