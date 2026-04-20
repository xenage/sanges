use std::path::Path;

use uuid::Uuid;

use crate::config::{HardeningConfig, IsolationMode, RuntimeConfig, SandboxPolicy};
use crate::{Result, SandboxError};

#[derive(Debug, Clone, Default)]
pub struct HardeningStatus {
    pub warnings: Vec<String>,
}

pub async fn preflight_runtime(config: &RuntimeConfig) -> Result<HardeningStatus> {
    if config.isolation_mode != IsolationMode::Secure {
        return Ok(HardeningStatus::default());
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = config;
        Err(SandboxError::UnsupportedHost(
            "secure host isolation requires Linux".into(),
        ))
    }

    #[cfg(target_os = "linux")]
    {
        let parent = config.hardening.cgroup_parent.as_ref().ok_or_else(|| {
            SandboxError::invalid("secure isolation mode requires SAGENS_CGROUP_PARENT")
        })?;
        validate_cgroup_parent(parent).await?;

        let mut status = HardeningStatus::default();
        if config.hardening.enable_landlock {
            status.warnings.push(
                "landlock support is not yet enabled; secure mode currently relies on cgroup and process isolation".into(),
            );
        }
        Ok(status)
    }
}

pub async fn attach_backend_process(
    config: &HardeningConfig,
    policy: &SandboxPolicy,
    sandbox_id: Uuid,
    pid: u32,
) -> Result<HardeningStatus> {
    if let Some(parent) = &config.cgroup_parent {
        attach_to_cgroup(parent, sandbox_id, pid, policy).await?;
    }

    let mut status = HardeningStatus::default();
    if config.enable_landlock && !cfg!(target_os = "linux") {
        status
            .warnings
            .push("landlock requested on a non-Linux host; running without it".into());
    }
    Ok(status)
}

async fn attach_to_cgroup(
    parent: &Path,
    sandbox_id: Uuid,
    pid: u32,
    policy: &SandboxPolicy,
) -> Result<()> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (parent, sandbox_id, pid, policy);
        Err(SandboxError::UnsupportedHost(
            "cgroup hardening requires Linux".into(),
        ))
    }

    #[cfg(target_os = "linux")]
    {
        use tokio::fs;

        let dir = parent.join(sandbox_id.to_string());
        fs::create_dir_all(&dir)
            .await
            .map_err(|error| SandboxError::io("creating sandbox cgroup directory", error))?;
        fs::write(
            dir.join("memory.max"),
            format!("{}\n", (policy.memory_mb as u64) * 1024 * 1024),
        )
        .await
        .map_err(|error| SandboxError::io("writing sandbox memory.max", error))?;
        fs::write(
            dir.join("cpu.max"),
            format!("{} 100000\n", u64::from(policy.cpu_cores) * 100_000),
        )
        .await
        .map_err(|error| SandboxError::io("writing sandbox cpu.max", error))?;
        fs::write(
            dir.join("pids.max"),
            format!("{}\n", policy.max_processes.saturating_add(32)),
        )
        .await
        .map_err(|error| SandboxError::io("writing sandbox pids.max", error))?;
        fs::write(dir.join("cgroup.procs"), format!("{pid}\n"))
            .await
            .map_err(|error| SandboxError::io("attaching backend process to cgroup", error))
    }
}

#[cfg(target_os = "linux")]
async fn validate_cgroup_parent(parent: &std::path::Path) -> Result<()> {
    use tokio::fs;

    if !fs::try_exists(parent)
        .await
        .map_err(|error| SandboxError::io("checking delegated cgroup parent", error))?
    {
        return Err(SandboxError::invalid(format!(
            "delegated cgroup parent {} does not exist",
            parent.display()
        )));
    }

    let probe = parent.join(format!("sagens-preflight-{}", Uuid::new_v4().simple()));
    fs::create_dir(&probe)
        .await
        .map_err(|error| SandboxError::io("creating delegated cgroup probe", error))?;

    let result = async {
        let _ = fs::read_to_string(parent.join("cgroup.controllers"))
            .await
            .map_err(|error| SandboxError::io("reading delegated cgroup controllers", error))?;
        fs::write(probe.join("memory.max"), "max\n")
            .await
            .map_err(|error| SandboxError::io("writing delegated cgroup memory.max", error))?;
        fs::write(probe.join("cpu.max"), "max 100000\n")
            .await
            .map_err(|error| SandboxError::io("writing delegated cgroup cpu.max", error))?;
        fs::write(probe.join("pids.max"), "64\n")
            .await
            .map_err(|error| SandboxError::io("writing delegated cgroup pids.max", error))?;
        Result::<()>::Ok(())
    }
    .await;

    let cleanup = fs::remove_dir(&probe)
        .await
        .map_err(|error| SandboxError::io("removing delegated cgroup probe", error));
    result?;
    cleanup?;
    Ok(())
}
