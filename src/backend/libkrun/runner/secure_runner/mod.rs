#[cfg(target_os = "linux")]
mod bootstrap;
#[cfg(target_os = "linux")]
mod harness;
#[cfg(target_os = "linux")]
mod policies;
#[cfg(target_os = "linux")]
mod sys;

use crate::Result;
use crate::backend::libkrun::config::LibkrunRunnerConfig;

pub(super) fn apply_secure_bootstrap(config: &mut LibkrunRunnerConfig) -> Result<()> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = config;
        Err(crate::SandboxError::UnsupportedHost(
            "secure runner bootstrap requires Linux".into(),
        ))
    }

    #[cfg(target_os = "linux")]
    {
        bootstrap::apply_secure_bootstrap(config)
    }
}

pub(super) fn run_security_harness(config: &mut LibkrunRunnerConfig) -> Result<()> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = config;
        Err(crate::SandboxError::UnsupportedHost(
            "security harness requires Linux".into(),
        ))
    }

    #[cfg(target_os = "linux")]
    {
        bootstrap::apply_secure_bootstrap(config)?;
        harness::verify_security_harness()?;
        println!("security harness passed");
        Ok(())
    }
}
