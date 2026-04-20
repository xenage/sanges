use std::path::PathBuf;

use uuid::Uuid;

use super::format::{
    format_box_cpu_setting, format_box_fs_setting, format_box_memory_setting,
    format_box_network_setting, format_box_process_setting,
};
use crate::boxes::{
    BoxBooleanSetting, BoxNumericSetting, BoxRecord, BoxRuntimeUsage, BoxSettings, BoxStatus,
};

fn sample_settings() -> BoxSettings {
    BoxSettings {
        cpu_cores: BoxNumericSetting {
            current: 2,
            max: 10,
        },
        memory_mb: BoxNumericSetting {
            current: 2048,
            max: 16384,
        },
        fs_size_mib: BoxNumericSetting {
            current: 4096,
            max: 65536,
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

fn sample_box_record(status: BoxStatus, runtime_usage: Option<BoxRuntimeUsage>) -> BoxRecord {
    BoxRecord {
        box_id: Uuid::nil(),
        name: None,
        status,
        settings: Some(sample_settings()),
        runtime_usage,
        workspace_path: PathBuf::from("/workspace.raw"),
        active_sandbox_id: None,
        created_at_ms: 0,
        last_start_at_ms: None,
        last_stop_at_ms: None,
        last_error: None,
    }
}

#[test]
fn formats_running_resources_from_runtime_usage() {
    let record = sample_box_record(
        BoxStatus::Running,
        Some(BoxRuntimeUsage {
            cpu_millicores: 1250,
            memory_used_mib: 768,
            fs_used_mib: 640,
            process_count: 23,
        }),
    );
    let settings = record.settings.as_ref().expect("settings");

    assert_eq!(format_box_cpu_setting(&record, settings), "1.25 / 2");
    assert_eq!(
        format_box_memory_setting(&record, settings),
        "768MiB / 2GiB"
    );
    assert_eq!(format_box_fs_setting(&record, settings), "640MiB / 4GiB");
    assert_eq!(format_box_process_setting(&record, settings), "23 / 256");
}

#[test]
fn formats_running_resources_as_unknown_when_usage_is_missing() {
    let record = sample_box_record(BoxStatus::Running, None);
    let settings = record.settings.as_ref().expect("settings");

    assert_eq!(format_box_cpu_setting(&record, settings), "— / 2");
    assert_eq!(format_box_memory_setting(&record, settings), "— / 2GiB");
    assert_eq!(format_box_fs_setting(&record, settings), "— / 4GiB");
    assert_eq!(format_box_process_setting(&record, settings), "— / 256");
}

#[test]
fn formats_inactive_resources_as_zero_over_configured_limit() {
    let record = sample_box_record(BoxStatus::Stopped, None);
    let settings = record.settings.as_ref().expect("settings");

    assert_eq!(format_box_cpu_setting(&record, settings), "0 / 2");
    assert_eq!(format_box_memory_setting(&record, settings), "0MiB / 2GiB");
    assert_eq!(format_box_fs_setting(&record, settings), "0MiB / 4GiB");
    assert_eq!(format_box_process_setting(&record, settings), "0 / 256");
}

#[test]
fn formats_network_from_vm_config_only() {
    let setting = BoxBooleanSetting {
        current: false,
        max: true,
    };

    assert_eq!(format_box_network_setting(&setting), "off");
}
