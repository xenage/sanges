use crate::config::SandboxPolicy;
use crate::{Result, SandboxError};

use super::super::BoxRecord;

pub(super) fn validate_numeric_setting<T>(name: &str, value: T, max: T, min: T) -> Result<()>
where
    T: Copy + Ord + std::fmt::Display,
{
    if value < min {
        return Err(SandboxError::invalid(format!(
            "{name} must be at least {min}"
        )));
    }
    if value > max {
        return Err(SandboxError::invalid(format!("{name} cannot exceed {max}")));
    }
    Ok(())
}

pub(super) fn box_policy(record: &BoxRecord, timeout_ms: Option<u64>) -> Result<SandboxPolicy> {
    let settings = record.settings.as_ref().ok_or_else(|| {
        SandboxError::backend(format!(
            "BOX {} is missing persisted settings",
            record.box_id
        ))
    })?;
    Ok(SandboxPolicy {
        cpu_cores: settings.cpu_cores.current,
        memory_mb: settings.memory_mb.current,
        max_processes: settings.max_processes.current,
        network_enabled: settings.network_enabled.current,
        timeout_ms,
    })
}
