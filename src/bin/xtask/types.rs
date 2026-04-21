use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use serde::Serialize;

pub(super) const EMBED_MANIFEST: &str = ".sagens-state/embed-manifest.json";
pub(super) const GUEST_AGENT_MANIFEST: &str = "crates/sagens-guest-agent/Cargo.toml";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Profile {
    Debug,
    Release,
}

impl Profile {
    pub(super) fn parse(value: &str) -> anyhow::Result<Self> {
        match value {
            "debug" => Ok(Self::Debug),
            "release" => Ok(Self::Release),
            _ => bail!("unsupported profile: {value}"),
        }
    }

    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Release => "release",
        }
    }

    pub(super) fn cargo_flag(self) -> Option<&'static str> {
        match self {
            Self::Debug => None,
            Self::Release => Some("--release"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PlatformOs {
    Macos,
    Linux,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PlatformArch {
    Aarch64,
    X86_64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Platform {
    pub(super) os: PlatformOs,
    pub(super) arch: PlatformArch,
}

impl Platform {
    pub(super) fn current() -> anyhow::Result<Self> {
        let os = match env::consts::OS {
            "macos" => PlatformOs::Macos,
            "linux" => PlatformOs::Linux,
            other => bail!("unsupported host operating system: {other}"),
        };
        let arch = match env::consts::ARCH {
            "aarch64" => PlatformArch::Aarch64,
            "x86_64" => PlatformArch::X86_64,
            other => bail!("unsupported host architecture: {other}"),
        };
        Ok(Self { os, arch })
    }

    pub(super) fn parse(value: &str) -> anyhow::Result<Self> {
        match value {
            "macos-aarch64" => Ok(Self {
                os: PlatformOs::Macos,
                arch: PlatformArch::Aarch64,
            }),
            "macos-x86_64" => Ok(Self {
                os: PlatformOs::Macos,
                arch: PlatformArch::X86_64,
            }),
            "linux-aarch64" => Ok(Self {
                os: PlatformOs::Linux,
                arch: PlatformArch::Aarch64,
            }),
            "linux-x86_64" => Ok(Self {
                os: PlatformOs::Linux,
                arch: PlatformArch::X86_64,
            }),
            _ => bail!("unsupported platform: {value}"),
        }
    }

    pub(super) fn as_str(self) -> &'static str {
        match (self.os, self.arch) {
            (PlatformOs::Macos, PlatformArch::Aarch64) => "macos-aarch64",
            (PlatformOs::Macos, PlatformArch::X86_64) => "macos-x86_64",
            (PlatformOs::Linux, PlatformArch::Aarch64) => "linux-aarch64",
            (PlatformOs::Linux, PlatformArch::X86_64) => "linux-x86_64",
        }
    }

    pub(super) fn guest_arch(self) -> &'static str {
        match self.arch {
            PlatformArch::Aarch64 => "aarch64",
            PlatformArch::X86_64 => "x86_64",
        }
    }

    pub(super) fn guest_target(self) -> &'static str {
        match self.arch {
            PlatformArch::Aarch64 => "aarch64-unknown-linux-musl",
            PlatformArch::X86_64 => "x86_64-unknown-linux-musl",
        }
    }

    pub(super) fn lib_name(self) -> &'static str {
        match self.os {
            PlatformOs::Macos => "libkrun.dylib",
            PlatformOs::Linux => "libkrun.so",
        }
    }
}

#[derive(Debug)]
pub(super) struct RuntimeBundle {
    pub(super) bundle_dir: PathBuf,
    pub(super) libkrun: PathBuf,
    pub(super) firmware: Option<PathBuf>,
    pub(super) runtime_support: Vec<PathBuf>,
    pub(super) source: RuntimeBundleSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RuntimeBundleSource {
    Prebuilt,
    SourceBuild,
}

impl RuntimeBundleSource {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Prebuilt => "prebuilt",
            Self::SourceBuild => "built-from-source",
        }
    }
}

#[derive(Debug, Serialize)]
struct EmbedManifest {
    libkrun: Option<PathBuf>,
    kernel: PathBuf,
    rootfs: PathBuf,
    firmware: Option<PathBuf>,
    runtime_support: Vec<PathBuf>,
}

pub(super) struct ResolvedArtifacts {
    pub(super) libkrun: Option<PathBuf>,
    pub(super) kernel: PathBuf,
    pub(super) rootfs: PathBuf,
    pub(super) firmware: Option<PathBuf>,
    pub(super) runtime_support: Vec<PathBuf>,
}

pub(super) struct EmbedManifestGuard {
    path: PathBuf,
}

impl EmbedManifestGuard {
    pub(super) fn write(path: &Path, artifacts: &ResolvedArtifacts) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        let manifest = EmbedManifest {
            libkrun: artifacts.libkrun.clone(),
            kernel: artifacts.kernel.clone(),
            rootfs: artifacts.rootfs.clone(),
            firmware: artifacts.firmware.clone(),
            runtime_support: artifacts.runtime_support.clone(),
        };
        let bytes = serde_json::to_vec_pretty(&manifest).context("encoding embed manifest")?;
        fs::write(path, bytes).with_context(|| format!("writing {}", path.display()))?;
        Ok(Self {
            path: path.to_path_buf(),
        })
    }
}

impl Drop for EmbedManifestGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

pub(super) fn repo_root() -> anyhow::Result<PathBuf> {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .context("xtask manifest directory has no workspace root parent")
}

pub(super) fn target_root(root: &Path) -> PathBuf {
    env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .map(|path| absolutize(root, &path))
        .unwrap_or_else(|| root.join("target"))
}

pub(super) fn absolutize(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}
