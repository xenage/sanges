use std::ffi::CString;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::thread;
use std::time::Duration;

use crate::guest_rpc::GuestRuntimeStats;
use crate::{Result, SandboxError};

const CPU_SAMPLE_WINDOW_MS: u64 = 120;
const WORKSPACE_ROOT: &str = "/workspace";

pub async fn runtime_stats() -> Result<GuestRuntimeStats> {
    tokio::task::spawn_blocking(collect_runtime_stats)
        .await
        .map_err(|error| SandboxError::backend(format!("joining runtime stats task: {error}")))?
}

fn collect_runtime_stats() -> Result<GuestRuntimeStats> {
    let first = read_cpu_sample()?;
    thread::sleep(Duration::from_millis(CPU_SAMPLE_WINDOW_MS));
    let second = read_cpu_sample()?;
    let cpu_count = std::thread::available_parallelism()
        .map(|value| value.get() as u32)
        .unwrap_or(1)
        .max(1);
    Ok(GuestRuntimeStats {
        cpu_millicores: cpu_millicores_used(first, second, cpu_count),
        memory_used_mib: read_memory_used_mib()?,
        fs_used_mib: read_fs_used_mib(Path::new(WORKSPACE_ROOT))?,
        process_count: read_process_count()?,
    })
}

#[derive(Clone, Copy)]
struct CpuSample {
    busy: u64,
    total: u64,
}

fn read_cpu_sample() -> Result<CpuSample> {
    let stat = fs::read_to_string("/proc/stat")
        .map_err(|error| SandboxError::io("reading /proc/stat", error))?;
    let line = stat
        .lines()
        .next()
        .ok_or_else(|| SandboxError::protocol("missing aggregate cpu line in /proc/stat"))?;
    let mut fields = line.split_whitespace();
    let Some("cpu") = fields.next() else {
        return Err(SandboxError::protocol(
            "unexpected aggregate cpu format in /proc/stat",
        ));
    };
    let values = fields
        .take(8)
        .map(|value| {
            value.parse::<u64>().map_err(|error| {
                SandboxError::protocol(format!("invalid cpu stat value `{value}`: {error}"))
            })
        })
        .collect::<Result<Vec<_>>>()?;
    if values.len() < 8 {
        return Err(SandboxError::protocol(
            "expected at least 8 aggregate cpu counters in /proc/stat",
        ));
    }
    let idle = values[3].saturating_add(values[4]);
    let total = values.iter().copied().sum::<u64>();
    Ok(CpuSample {
        busy: total.saturating_sub(idle),
        total,
    })
}

fn cpu_millicores_used(first: CpuSample, second: CpuSample, cpu_count: u32) -> u32 {
    let busy_delta = second.busy.saturating_sub(first.busy) as u128;
    let total_delta = second.total.saturating_sub(first.total) as u128;
    if total_delta == 0 {
        return 0;
    }
    let millicores = (busy_delta
        .saturating_mul(cpu_count as u128)
        .saturating_mul(1000)
        .saturating_add(total_delta / 2))
        / total_delta;
    millicores.min((cpu_count as u128).saturating_mul(1000)) as u32
}

fn read_memory_used_mib() -> Result<u64> {
    let meminfo = fs::read_to_string("/proc/meminfo")
        .map_err(|error| SandboxError::io("reading /proc/meminfo", error))?;
    let mut total_kib = None;
    let mut available_kib = None;
    for line in meminfo.lines() {
        if let Some(value) = line.strip_prefix("MemTotal:") {
            total_kib = Some(parse_meminfo_kib(value, "MemTotal")?);
        } else if let Some(value) = line.strip_prefix("MemAvailable:") {
            available_kib = Some(parse_meminfo_kib(value, "MemAvailable")?);
        }
    }
    let total_kib =
        total_kib.ok_or_else(|| SandboxError::protocol("MemTotal missing from /proc/meminfo"))?;
    let available_kib = available_kib
        .ok_or_else(|| SandboxError::protocol("MemAvailable missing from /proc/meminfo"))?;
    Ok(total_kib.saturating_sub(available_kib) / 1024)
}

fn parse_meminfo_kib(raw: &str, label: &str) -> Result<u64> {
    let value = raw
        .trim()
        .strip_suffix(" kB")
        .ok_or_else(|| SandboxError::protocol(format!("{label} is missing kB suffix")))?;
    value.trim().parse::<u64>().map_err(|error| {
        SandboxError::protocol(format!("invalid {label} value `{value}`: {error}"))
    })
}

fn read_process_count() -> Result<u32> {
    let entries =
        fs::read_dir("/proc").map_err(|error| SandboxError::io("reading /proc", error))?;
    let mut count = 0_u32;
    for entry in entries {
        let entry = entry.map_err(|error| SandboxError::io("iterating /proc", error))?;
        if entry
            .file_name()
            .to_string_lossy()
            .bytes()
            .all(|value| value.is_ascii_digit())
        {
            count = count.saturating_add(1);
        }
    }
    Ok(count)
}

fn read_fs_used_mib(path: &Path) -> Result<u64> {
    let path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| SandboxError::backend("invalid workspace path"))?;
    let mut stats = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    let result = unsafe { libc::statvfs(path.as_ptr(), stats.as_mut_ptr()) };
    if result != 0 {
        return Err(SandboxError::io(
            "reading workspace filesystem stats",
            std::io::Error::last_os_error(),
        ));
    }
    let stats = unsafe { stats.assume_init() };
    let total_bytes = (stats.f_blocks as u128).saturating_mul(stats.f_frsize as u128);
    let avail_bytes = (stats.f_bavail as u128).saturating_mul(stats.f_frsize as u128);
    Ok(((total_bytes.saturating_sub(avail_bytes)) / (1024 * 1024) as u128) as u64)
}

#[cfg(test)]
mod tests {
    use super::CpuSample;
    use super::cpu_millicores_used;

    #[test]
    fn cpu_usage_scales_to_used_cores() {
        let first = CpuSample {
            busy: 100,
            total: 200,
        };
        let second = CpuSample {
            busy: 250,
            total: 400,
        };

        assert_eq!(cpu_millicores_used(first, second, 2), 1500);
    }
}
