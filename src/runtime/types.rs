use crate::config::SandboxPolicy;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxSessionState {
    Active,
    Destroyed,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SandboxSessionSummary {
    pub sandbox_id: Uuid,
    pub workspace_id: String,
    pub state: SandboxSessionState,
    pub policy: SandboxPolicy,
    pub started_at_ms: u64,
    pub ended_at_ms: Option<u64>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SandboxSessionRecord {
    pub summary: SandboxSessionSummary,
    pub changes: Vec<crate::workspace::WorkspaceChange>,
}
