#[path = "runtime/guest_fingerprint.rs"]
mod guest_fingerprint;
#[path = "runtime/libkrunfw_kernel.rs"]
mod libkrunfw_kernel;
#[path = "runtime/submodules.rs"]
mod submodules;

use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, ensure};

use super::cargo_ops::{cargo_build, run};
use super::types::{
    GUEST_AGENT_MANIFEST, Platform, PlatformArch, PlatformOs, Profile, ResolvedArtifacts,
    absolutize, target_root,
};
pub(super) use guest_fingerprint::{guest_artifacts_stale, write_guest_artifact_fingerprint};
use submodules::ensure_upstream_checkout;

const GUEST_OUTPUT_DIR_ENV: &str = "SAGENS_GUEST_OUTPUT_DIR";
const GUEST_WORK_DIR_ENV: &str = "SAGENS_GUEST_WORK_DIR";

pub(super) fn maybe_init_submodules(root: &Path) -> anyhow::Result<()> {
    submodules::maybe_init_submodules(root)
}

pub(super) fn build_guest_artifacts(
    root: &Path,
    platform: Platform,
    profile: Profile,
) -> anyhow::Result<()> {
    build_guest_agent(root, platform, profile)?;
    cargo_build(root, profile, &["build-alpine-guest"])?;
    let builder = target_root(root)
        .join(profile.as_str())
        .join("build-alpine-guest");
    let guest = guest_agent_binary(root, platform, profile);
    let output_dir = guest_output_dir(root, platform);
    let work_dir = guest_work_dir(root);
    let mut command = Command::new(&builder);
    command
        .arg("--arch")
        .arg(platform.guest_arch())
        .arg("--work-dir")
        .arg(&work_dir)
        .arg("--guest-agent")
        .arg(&guest)
        .arg("--output-dir")
        .arg(&output_dir);
    run(command, "building Alpine guest artifacts")?;
    match (platform.os, platform.arch) {
        (PlatformOs::Macos, PlatformArch::Aarch64) => {
            libkrunfw_kernel::materialize_macos_aarch64_guest_kernel(&work_dir, &output_dir)?;
        }
        _ => {}
    }
    Ok(())
}

pub(super) fn resolve_artifacts(
    root: &Path,
    platform: Platform,
) -> anyhow::Result<ResolvedArtifacts> {
    let kernel = guest_kernel_path(root, platform);
    let rootfs = guest_rootfs_path(root, platform);
    ensure!(
        kernel.is_file(),
        "missing guest kernel: {}",
        kernel.display()
    );
    ensure!(
        rootfs.is_file(),
        "missing guest rootfs: {}",
        rootfs.display()
    );
    Ok(ResolvedArtifacts {
        kernel,
        rootfs,
        firmware: resolve_firmware(root, platform)?,
    })
}

pub(super) fn guest_kernel_path(root: &Path, platform: Platform) -> PathBuf {
    guest_output_dir(root, platform).join("vmlinuz-virt")
}

pub(super) fn guest_rootfs_path(root: &Path, platform: Platform) -> PathBuf {
    guest_output_dir(root, platform).join("rootfs.raw")
}

fn resolve_firmware(root: &Path, platform: Platform) -> anyhow::Result<Option<PathBuf>> {
    if platform.os != PlatformOs::Macos {
        return Ok(None);
    }
    let libkrun_root = ensure_upstream_checkout(root, "third_party/upstream/libkrun", "Makefile")?;
    let firmware = libkrun_root.join("edk2").join("KRUN_EFI.silent.fd");
    ensure!(
        firmware.is_file(),
        "missing libkrun firmware: {}",
        firmware.display()
    );
    Ok(Some(firmware))
}

fn build_guest_agent(root: &Path, platform: Platform, profile: Profile) -> anyhow::Result<()> {
    ensure_rust_target(platform.guest_target())?;
    let target_dir = target_root(root).join("guest-agent");
    let mut command = crate::cmd::tool_command("cargo");
    command
        .arg("build")
        .arg("--manifest-path")
        .arg(root.join(GUEST_AGENT_MANIFEST))
        .arg("--target")
        .arg(platform.guest_target())
        .arg("--target-dir")
        .arg(&target_dir);
    if platform.guest_target() == "aarch64-unknown-linux-musl" {
        command.env("CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER", "rust-lld");
    }
    if platform.guest_target() == "x86_64-unknown-linux-musl" {
        command.env("CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER", "rust-lld");
    }
    if let Some(flag) = profile.cargo_flag() {
        command.arg(flag);
    }
    command.current_dir(root);
    run(command, "building guest agent binary")
}

fn guest_output_dir(root: &Path, platform: Platform) -> PathBuf {
    env::var_os(GUEST_OUTPUT_DIR_ENV)
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .map(|path| absolutize(root, &path))
        .unwrap_or_else(|| {
            root.join("artifacts")
                .join(format!("alpine-{}", platform.guest_arch()))
        })
}

fn guest_work_dir(root: &Path) -> PathBuf {
    env::var_os(GUEST_WORK_DIR_ENV)
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .map(|path| absolutize(root, &path))
        .unwrap_or_else(|| root.join(".box-artifacts"))
}

fn guest_agent_binary(root: &Path, platform: Platform, profile: Profile) -> PathBuf {
    target_root(root)
        .join("guest-agent")
        .join(platform.guest_target())
        .join(profile.as_str())
        .join("sagens-guest-agent")
}

fn ensure_rust_target(target: &str) -> anyhow::Result<()> {
    let status = crate::cmd::tool_command("rustup")
        .arg("target")
        .arg("add")
        .arg(target)
        .stdin(Stdio::null())
        .status()
        .with_context(|| format!("ensuring rust target {target}"))?;
    if !status.success() {
        anyhow::bail!("rustup target add {target} failed with status {status}");
    }
    Ok(())
}
