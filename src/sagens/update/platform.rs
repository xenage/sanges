use crate::{Result, SandboxError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TargetPlatform {
    LinuxX86_64,
    LinuxAarch64,
    MacosAarch64,
}

impl TargetPlatform {
    pub(super) fn detect() -> Result<Self> {
        platform_from_parts(std::env::consts::OS, std::env::consts::ARCH)
    }

    pub(super) fn slug(self) -> &'static str {
        match self {
            Self::LinuxX86_64 => "linux-x86_64",
            Self::LinuxAarch64 => "linux-aarch64",
            Self::MacosAarch64 => "macos-aarch64",
        }
    }
}

pub(super) fn platform_from_parts(os: &str, arch: &str) -> Result<TargetPlatform> {
    match (os, arch) {
        ("linux", "x86_64") | ("linux", "amd64") => Ok(TargetPlatform::LinuxX86_64),
        ("linux", "aarch64") | ("linux", "arm64") => Ok(TargetPlatform::LinuxAarch64),
        ("macos", "x86_64") | ("darwin", "x86_64") => Err(SandboxError::invalid(
            "self-update is not supported on macOS x86_64 because Intel Mac builds are no longer published",
        )),
        ("macos", "aarch64") | ("macos", "arm64") | ("darwin", "aarch64") | ("darwin", "arm64") => {
            Ok(TargetPlatform::MacosAarch64)
        }
        _ => Err(SandboxError::invalid(format!(
            "self-update is not supported on platform {os}/{arch}"
        ))),
    }
}
