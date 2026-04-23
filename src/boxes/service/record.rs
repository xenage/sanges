use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;
use uuid::Uuid;

use crate::boxes::helpers::now_ms;
use crate::config::{IsolationMode, SandboxPolicy};
use crate::runtime::SandboxService;
use crate::workspace::WorkspaceStore;
use crate::{Result, WorkspaceConfig};

use super::super::{BoxRecord, BoxStatus, BoxStore};
use super::LocalBoxService;

impl LocalBoxService {
    pub async fn new(
        state_dir: impl Into<std::path::PathBuf>,
        workspace_config: WorkspaceConfig,
        default_policy: SandboxPolicy,
        isolation_mode: IsolationMode,
        runtime: Arc<dyn SandboxService>,
    ) -> Result<Self> {
        let state_dir = state_dir.into();
        let boxes = BoxStore::new(&state_dir);
        boxes.ensure_layout().await?;
        let workspace = WorkspaceStore::new(&state_dir, workspace_config.clone());
        workspace.ensure_layout().await?;
        let service = Self {
            state_dir,
            runtime,
            boxes,
            workspace,
            workspace_config,
            default_policy,
            isolation_mode,
            active: RwLock::new(HashMap::new()),
        };
        service.reconcile_after_restart().await?;
        Ok(service)
    }

    pub(super) async fn create_box_record(
        &self,
        name: Option<String>,
        settings: Option<super::super::BoxSettings>,
    ) -> Result<BoxRecord> {
        let box_id = Uuid::new_v4();
        let workspace = self
            .workspace
            .prepare_workspace(&box_id.to_string())
            .await?;
        let record = BoxRecord {
            box_id,
            name,
            status: BoxStatus::Created,
            settings,
            runtime_usage: None,
            workspace_path: workspace.disk_path,
            active_sandbox_id: None,
            created_at_ms: now_ms(),
            last_start_at_ms: None,
            last_stop_at_ms: None,
            last_error: None,
        };
        let record = self.hydrate_record(record).await?;
        self.boxes.write(&record).await?;
        Ok(record)
    }
}
