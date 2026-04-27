use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use sagens_host::boxes::{BoxBooleanSetting, BoxNumericSetting, BoxRecord, BoxSettings};
use sagens_host::workspace::{FileNode, WorkspaceCheckpointRecord};
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(Default)]
pub(super) struct StubState {
    pub(super) boxes: HashMap<Uuid, BoxRecord>,
    pub(super) files: HashMap<Uuid, Vec<FileNode>>,
    pub(super) file_data: HashMap<(Uuid, String), Vec<u8>>,
    pub(super) committed: u64,
    pub(super) checkpoints: HashMap<Uuid, Vec<WorkspaceCheckpointRecord>>,
    pub(super) checkpoint_snapshots: HashMap<(Uuid, String), BTreeMap<String, Vec<u8>>>,
    pub(super) checkpoint_heads: HashMap<Uuid, String>,
}

pub(crate) struct StubBoxManager {
    pub(super) state: Mutex<StubState>,
    pub(super) exec_running: Arc<Mutex<HashSet<Uuid>>>,
}

impl Default for StubBoxManager {
    fn default() -> Self {
        Self {
            state: Mutex::new(StubState::default()),
            exec_running: Arc::new(Mutex::new(HashSet::new())),
        }
    }
}

pub(super) fn default_box_settings() -> BoxSettings {
    BoxSettings {
        cpu_cores: BoxNumericSetting { current: 1, max: 8 },
        memory_mb: BoxNumericSetting {
            current: 128,
            max: 8192,
        },
        fs_size_mib: BoxNumericSetting {
            current: 128,
            max: 8192,
        },
        max_processes: BoxNumericSetting {
            current: 256,
            max: 4096,
        },
        network_enabled: BoxBooleanSetting {
            current: false,
            max: true,
        },
    }
}
