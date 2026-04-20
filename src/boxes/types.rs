use std::path::PathBuf;

use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BoxNumericSetting<T> {
    pub current: T,
    pub max: T,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BoxBooleanSetting {
    pub current: bool,
    pub max: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BoxSettings {
    pub cpu_cores: BoxNumericSetting<u32>,
    pub memory_mb: BoxNumericSetting<u32>,
    pub fs_size_mib: BoxNumericSetting<u64>,
    pub max_processes: BoxNumericSetting<u32>,
    pub network_enabled: BoxBooleanSetting,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BoxRuntimeUsage {
    pub cpu_millicores: u32,
    pub memory_used_mib: u64,
    pub fs_used_mib: u64,
    pub process_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "setting", rename_all = "snake_case")]
pub enum BoxSettingValue {
    CpuCores { value: u32 },
    MemoryMb { value: u32 },
    FsSizeMib { value: u64 },
    MaxProcesses { value: u32 },
    NetworkEnabled { value: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoxStatus {
    Created,
    Running,
    Stopped,
    Failed,
    Removing,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BoxRecord {
    pub box_id: Uuid,
    #[serde(default)]
    pub name: Option<String>,
    pub status: BoxStatus,
    #[serde(default)]
    pub settings: Option<BoxSettings>,
    #[serde(default)]
    pub runtime_usage: Option<BoxRuntimeUsage>,
    pub workspace_path: PathBuf,
    pub active_sandbox_id: Option<Uuid>,
    pub created_at_ms: u64,
    pub last_start_at_ms: Option<u64>,
    pub last_stop_at_ms: Option<u64>,
    pub last_error: Option<String>,
}

impl From<crate::guest_rpc::GuestRuntimeStats> for BoxRuntimeUsage {
    fn from(value: crate::guest_rpc::GuestRuntimeStats) -> Self {
        Self {
            cpu_millicores: value.cpu_millicores,
            memory_used_mib: value.memory_used_mib,
            fs_used_mib: value.fs_used_mib,
            process_count: value.process_count,
        }
    }
}
