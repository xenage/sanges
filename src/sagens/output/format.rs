use crate::boxes::{
    BoxBooleanSetting, BoxNumericSetting, BoxRecord, BoxRuntimeUsage, BoxSettings, BoxStatus,
};

pub(super) fn format_box_network_setting(setting: &BoxBooleanSetting) -> String {
    format_bool(setting.current).to_string()
}

fn format_bool(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}

enum Unit {
    Count,
    Memory,
    Storage,
}

pub(super) fn format_box_cpu_setting(box_record: &BoxRecord, settings: &BoxSettings) -> String {
    let current = match current_runtime_usage(box_record) {
        Some(runtime) => format_cpu_millicores(runtime.cpu_millicores),
        None if box_record.status == BoxStatus::Running => "—".into(),
        None => "0".into(),
    };
    let max = settings.cpu_cores.current.to_string();
    format!("{current} / {max}")
}

pub(super) fn format_box_memory_setting(box_record: &BoxRecord, settings: &BoxSettings) -> String {
    format_box_usage_value(
        box_record,
        settings.memory_mb.current as u64,
        Unit::Memory,
        |runtime| runtime.memory_used_mib,
    )
}

pub(super) fn format_box_fs_setting(box_record: &BoxRecord, settings: &BoxSettings) -> String {
    format_box_usage_value(
        box_record,
        settings.fs_size_mib.current,
        Unit::Storage,
        |runtime| runtime.fs_used_mib,
    )
}

pub(super) fn format_box_process_setting(box_record: &BoxRecord, settings: &BoxSettings) -> String {
    format_box_usage_value(
        box_record,
        settings.max_processes.current as u64,
        Unit::Count,
        |runtime| runtime.process_count as u64,
    )
}

fn format_box_usage_value(
    box_record: &BoxRecord,
    configured_value: u64,
    unit: Unit,
    runtime_value: impl Fn(&BoxRuntimeUsage) -> u64,
) -> String {
    let max = format_numeric_value(configured_value, &unit);
    let current = match current_runtime_usage(box_record) {
        Some(runtime) => format_numeric_value(runtime_value(runtime), &unit),
        None if box_record.status == BoxStatus::Running => "—".into(),
        None => format_numeric_value(0, &unit),
    };
    format!("{current} / {max}")
}

fn current_runtime_usage(box_record: &BoxRecord) -> Option<&BoxRuntimeUsage> {
    (box_record.status == BoxStatus::Running)
        .then_some(box_record.runtime_usage.as_ref())
        .flatten()
}

fn format_numeric_value(value: u64, unit: &Unit) -> String {
    match unit {
        Unit::Count => value.to_string(),
        Unit::Memory | Unit::Storage => format_size_mib(value),
    }
}

fn format_cpu_millicores(value: u32) -> String {
    let hundredths = (value.saturating_add(5)) / 10;
    let whole = hundredths / 100;
    let fraction = hundredths % 100;
    if fraction == 0 {
        whole.to_string()
    } else if fraction.is_multiple_of(10) {
        format!("{whole}.{}", fraction / 10)
    } else {
        format!("{whole}.{fraction:02}")
    }
}

pub(super) fn format_size_mib(value: u64) -> String {
    if value >= 1024 && value.is_multiple_of(1024) {
        format!("{}GiB", value / 1024)
    } else if value >= 1024 {
        format!("{:.1}GiB", value as f64 / 1024.0)
    } else {
        format!("{value}MiB")
    }
}

pub(super) fn format_bytes(value: u64) -> String {
    if value >= 1024 * 1024 * 1024 {
        format!("{:.1} GiB", value as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if value >= 1024 * 1024 {
        format!("{:.1} MiB", value as f64 / (1024.0 * 1024.0))
    } else if value >= 1024 {
        format!("{:.1} KiB", value as f64 / 1024.0)
    } else {
        format!("{value} B")
    }
}

pub(super) fn fallback_settings() -> BoxSettings {
    BoxSettings {
        cpu_cores: BoxNumericSetting { current: 1, max: 1 },
        memory_mb: BoxNumericSetting {
            current: 512,
            max: 512,
        },
        fs_size_mib: BoxNumericSetting {
            current: 512,
            max: 512,
        },
        max_processes: BoxNumericSetting {
            current: 256,
            max: 256,
        },
        network_enabled: BoxBooleanSetting {
            current: false,
            max: false,
        },
    }
}
