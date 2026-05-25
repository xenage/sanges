use sagens_host::boxes::{BoxRecord, BoxSettings, BoxStatus};
use uuid::Uuid;

pub(super) fn new_box_record(name: Option<String>) -> BoxRecord {
    BoxRecord {
        box_id: Uuid::new_v4(),
        name,
        image: sagens_host::images::BASE_IMAGE_NAME.into(),
        status: BoxStatus::Created,
        settings: BoxSettings::default(),
        runtime_usage: None,
        workspace_path: "/tmp/workspace.raw".into(),
        active_sandbox_id: None,
        created_at_ms: 1,
        last_start_at_ms: None,
        last_stop_at_ms: None,
        last_error: None,
    }
}
