use std::time::{SystemTime, UNIX_EPOCH};

use uuid::Uuid;

use crate::host_log;
use crate::{Result, SandboxError};

use super::service::{BoxManager, LocalBoxService};
use super::{
    BoxBooleanSetting, BoxNumericSetting, BoxRecord, BoxRuntimeUsage, BoxSettings, BoxStatus,
};

impl LocalBoxService {
    pub(super) async fn reconcile_after_restart(&self) -> Result<()> {
        for record in self.boxes.list().await? {
            let mut record = self.hydrate_record(record).await?;
            if record.status == BoxStatus::Running {
                record.status = BoxStatus::Failed;
                record.last_error =
                    Some("daemon restarted while BOX was marked running; start it again".into());
                host_log::emit(
                    "box",
                    format!(
                        "marked failed after daemon restart box_id={} error={}",
                        record.box_id,
                        record.last_error.as_deref().unwrap_or("unknown")
                    ),
                );
                self.boxes.write(&record).await?;
            }
        }
        Ok(())
    }

    pub(super) async fn read_box(&self, box_id: Uuid) -> Result<BoxRecord> {
        let record = self.boxes.read(box_id).await?;
        self.hydrate_record(record).await
    }

    pub(super) async fn hydrate_record(&self, mut record: BoxRecord) -> Result<BoxRecord> {
        let original = record.clone();
        let actual_fs_size_mib = match self
            .workspace
            .workspace_disk_size_mib(&record.box_id.to_string())
            .await
        {
            Ok(size) => size,
            Err(_) => self.workspace_config.disk_size_mib,
        };
        let detected = detect_box_caps(
            &self.state_dir,
            self.default_policy,
            self.workspace_config.disk_size_mib,
            self.isolation_mode,
            actual_fs_size_mib,
        );
        record.settings = Some(normalize_settings(
            record.settings.take(),
            self.default_policy,
            self.workspace_config.disk_size_mib,
            actual_fs_size_mib,
            detected,
        ));
        if record != original {
            self.boxes.write(&record).await?;
        }
        Ok(record)
    }

    pub(super) async fn attach_runtime_usage(&self, mut record: BoxRecord) -> Result<BoxRecord> {
        record.runtime_usage = None;
        if record.status != BoxStatus::Running {
            return Ok(record);
        }
        let box_id = record.box_id;
        let sandbox_id = match record.active_sandbox_id {
            Some(sandbox_id) => Some(sandbox_id),
            None => self.active.read().await.get(&record.box_id).copied(),
        };
        let Some(sandbox_id) = sandbox_id else {
            self.mark_stopped(record).await?;
            return self.read_box(box_id).await;
        };
        match self.runtime.runtime_stats(sandbox_id).await {
            Ok(stats) => {
                record.runtime_usage = Some(BoxRuntimeUsage::from(stats));
                Ok(record)
            }
            Err(error) if missing_runtime_session(&error) => {
                self.mark_stopped(record).await?;
                self.read_box(box_id).await
            }
            Err(error) => {
                self.set_failed(record, error.to_string()).await?;
                self.read_box(box_id).await
            }
        }
    }

    pub(super) async fn running_sandbox_id(&self, box_id: Uuid) -> Result<Uuid> {
        let record = self.read_box(box_id).await?;
        if record.status != BoxStatus::Running {
            return Err(SandboxError::conflict(format!(
                "BOX {box_id} is not running"
            )));
        }
        self.active
            .read()
            .await
            .get(&box_id)
            .copied()
            .ok_or_else(|| SandboxError::backend(format!("BOX {box_id} is missing runtime state")))
    }

    pub(super) async fn set_failed(&self, mut record: BoxRecord, message: String) -> Result<()> {
        host_log::emit(
            "box",
            format!(
                "marked failed box_id={} sandbox_id={} error={message}",
                record.box_id,
                record
                    .active_sandbox_id
                    .map(|sandbox_id| sandbox_id.to_string())
                    .unwrap_or_else(|| "none".into())
            ),
        );
        record.status = BoxStatus::Failed;
        record.active_sandbox_id = None;
        record.last_error = Some(message);
        self.active.write().await.remove(&record.box_id);
        self.boxes.write(&record).await
    }

    pub(super) async fn active_box_ids(&self) -> Vec<Uuid> {
        let mut box_ids = self.active.read().await.keys().copied().collect::<Vec<_>>();
        box_ids.sort();
        box_ids
    }

    pub(super) async fn mark_stopped(&self, mut record: BoxRecord) -> Result<()> {
        record.status = BoxStatus::Stopped;
        record.active_sandbox_id = None;
        record.last_stop_at_ms = Some(now_ms());
        record.last_error = None;
        self.active.write().await.remove(&record.box_id);
        self.boxes.write(&record).await
    }

    pub(super) async fn ensure_runtime(&self, box_id: Uuid) -> Result<Uuid> {
        let record = self.read_box(box_id).await?;
        if record.status == BoxStatus::Running {
            if let Some(sandbox_id) = record.active_sandbox_id {
                self.active.write().await.insert(box_id, sandbox_id);
                if self.runtime.touch_session(sandbox_id).await.is_ok() {
                    return Ok(sandbox_id);
                }
            }
            self.mark_stopped(record).await?;
        }
        let record = self.start_box(box_id).await?;
        record.active_sandbox_id.ok_or_else(|| {
            SandboxError::backend(format!("BOX {box_id} started without an active sandbox"))
        })
    }

    pub(super) async fn stop_box_for_shutdown(&self, box_id: Uuid) -> Result<()> {
        let record = self.read_box(box_id).await?;
        if record.status != BoxStatus::Running {
            self.active.write().await.remove(&box_id);
            return Ok(());
        }
        let sandbox_id = match record.active_sandbox_id {
            Some(sandbox_id) => sandbox_id,
            None => match self.active.read().await.get(&box_id).copied() {
                Some(sandbox_id) => sandbox_id,
                None => {
                    self.mark_stopped(record).await?;
                    return Ok(());
                }
            },
        };
        if self.runtime.touch_session(sandbox_id).await.is_err() {
            self.mark_stopped(record).await?;
            return Ok(());
        }
        self.active.write().await.insert(box_id, sandbox_id);
        match self.runtime.destroy_sandbox(sandbox_id).await {
            Ok(_) => self.mark_stopped(record).await,
            Err(error) if missing_runtime_session(&error) => self.mark_stopped(record).await,
            Err(error) => {
                self.set_failed(record, error.to_string()).await?;
                Err(error)
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct BoxCaps {
    cpu_cores: u32,
    memory_mb: u32,
    fs_size_mib: u64,
    max_processes: u32,
    network_enabled: bool,
}

fn normalize_settings(
    existing: Option<BoxSettings>,
    default_policy: crate::config::SandboxPolicy,
    default_fs_size_mib: u64,
    actual_fs_size_mib: u64,
    detected: BoxCaps,
) -> BoxSettings {
    let mut settings = existing.unwrap_or(BoxSettings {
        cpu_cores: BoxNumericSetting {
            current: default_policy.cpu_cores,
            max: detected.cpu_cores,
        },
        memory_mb: BoxNumericSetting {
            current: default_policy.memory_mb,
            max: detected.memory_mb,
        },
        fs_size_mib: BoxNumericSetting {
            current: actual_fs_size_mib.max(default_fs_size_mib),
            max: detected.fs_size_mib,
        },
        max_processes: BoxNumericSetting {
            current: default_policy.max_processes,
            max: detected.max_processes,
        },
        network_enabled: BoxBooleanSetting {
            current: default_policy.network_enabled && detected.network_enabled,
            max: detected.network_enabled,
        },
    });

    settings.cpu_cores.max = detected.cpu_cores.max(settings.cpu_cores.current);
    settings.cpu_cores.current = settings.cpu_cores.current.clamp(1, settings.cpu_cores.max);

    settings.memory_mb.max = detected.memory_mb.max(settings.memory_mb.current);
    settings.memory_mb.current = settings
        .memory_mb
        .current
        .clamp(128, settings.memory_mb.max);

    settings.max_processes.max = detected.max_processes.max(settings.max_processes.current);
    settings.max_processes.current = settings
        .max_processes
        .current
        .clamp(1, settings.max_processes.max);

    settings.fs_size_mib.max = detected.fs_size_mib.max(actual_fs_size_mib);
    settings.fs_size_mib.current = actual_fs_size_mib.clamp(64, settings.fs_size_mib.max);

    settings.network_enabled.max = detected.network_enabled;
    settings.network_enabled.current &= settings.network_enabled.max;

    settings
}

fn detect_box_caps(
    state_dir: &std::path::Path,
    default_policy: crate::config::SandboxPolicy,
    default_fs_size_mib: u64,
    isolation_mode: crate::config::IsolationMode,
    actual_fs_size_mib: u64,
) -> BoxCaps {
    BoxCaps {
        cpu_cores: detect_cpu_cap(default_policy.cpu_cores),
        memory_mb: detect_memory_cap(default_policy.memory_mb),
        fs_size_mib: detect_fs_cap(state_dir, default_fs_size_mib, actual_fs_size_mib),
        max_processes: detect_process_cap(default_policy.max_processes),
        network_enabled: isolation_mode != crate::config::IsolationMode::Secure,
    }
}

fn detect_cpu_cap(default_value: u32) -> u32 {
    std::thread::available_parallelism()
        .map(|value| value.get() as u32)
        .unwrap_or(default_value.max(1))
        .max(default_value)
}

fn detect_memory_cap(default_value: u32) -> u32 {
    let fallback = default_value.max(4096);
    detect_total_memory_mib()
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(fallback)
        .max(default_value)
}

fn detect_process_cap(default_value: u32) -> u32 {
    detect_process_limit()
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(default_value.max(4096))
        .max(default_value)
}

fn detect_fs_cap(state_dir: &std::path::Path, default_value: u64, current_value: u64) -> u64 {
    detect_available_disk_mib(state_dir)
        .map(|free_mib| free_mib.saturating_add(current_value))
        .unwrap_or(default_value.saturating_mul(8).max(current_value))
        .max(default_value)
        .max(current_value)
}

#[cfg(unix)]
fn detect_total_memory_mib() -> Option<u64> {
    let pages = unsafe { libc::sysconf(libc::_SC_PHYS_PAGES) };
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if pages <= 0 || page_size <= 0 {
        return None;
    }
    Some((pages as u64).saturating_mul(page_size as u64) / (1024 * 1024))
}

#[cfg(not(unix))]
fn detect_total_memory_mib() -> Option<u64> {
    None
}

#[cfg(unix)]
fn detect_process_limit() -> Option<u64> {
    let mut limit = std::mem::MaybeUninit::<libc::rlimit>::uninit();
    let result = unsafe { libc::getrlimit(libc::RLIMIT_NPROC, limit.as_mut_ptr()) };
    if result != 0 {
        return None;
    }
    let limit = unsafe { limit.assume_init() };
    if limit.rlim_cur == libc::RLIM_INFINITY {
        return None;
    }
    Some(limit.rlim_cur.min(65_535))
}

#[cfg(not(unix))]
fn detect_process_limit() -> Option<u64> {
    None
}

#[cfg(unix)]
fn detect_available_disk_mib(path: &std::path::Path) -> Option<u64> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let path = CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut stats = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    let result = unsafe { libc::statvfs(path.as_ptr(), stats.as_mut_ptr()) };
    if result != 0 {
        return None;
    }
    let stats = unsafe { stats.assume_init() };
    let free_bytes = (stats.f_bavail as u128).saturating_mul(stats.f_frsize as u128);
    Some((free_bytes / (1024 * 1024) as u128) as u64)
}

#[cfg(not(unix))]
fn detect_available_disk_mib(_: &std::path::Path) -> Option<u64> {
    None
}

fn missing_runtime_session(error: &SandboxError) -> bool {
    match error {
        SandboxError::InvalidConfig(message) | SandboxError::Backend(message) => {
            message.contains("unknown active sandbox")
        }
        _ => false,
    }
}

pub(super) fn now_ms() -> u64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_millis() as u64,
        Err(_) => 0,
    }
}
