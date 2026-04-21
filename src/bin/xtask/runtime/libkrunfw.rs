use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, ensure};

use super::super::types::{Platform, PlatformArch, PlatformOs};
use super::submodules::ensure_upstream_checkout;

const LINUX_X86_64_LIBKRUNFW_NAME: &str = "libkrunfw.so.5";

pub(super) fn maybe_build_libkrunfw_support(
    root: &Path,
    platform: Platform,
    runtime_dir: &Path,
) -> anyhow::Result<()> {
    if !matches!(
        platform,
        Platform {
            os: PlatformOs::Linux,
            arch: PlatformArch::X86_64,
        }
    ) {
        return Ok(());
    }
    build_linux_x86_64_libkrunfw(root, runtime_dir)
}

pub(super) fn required_runtime_support_files(platform: Platform) -> &'static [&'static str] {
    match platform {
        Platform {
            os: PlatformOs::Linux,
            arch: PlatformArch::X86_64,
        } => &[LINUX_X86_64_LIBKRUNFW_NAME],
        _ => &[],
    }
}

fn build_linux_x86_64_libkrunfw(root: &Path, runtime_dir: &Path) -> anyhow::Result<()> {
    let libkrunfw_root =
        ensure_upstream_checkout(root, "third_party/upstream/libkrunfw", "Makefile")?;
    let mut make = crate::cmd::tool_command("make");
    make.arg("-C").arg(&libkrunfw_root);
    make.env_remove("CARGO_TARGET_DIR");
    super::super::cargo_ops::run(
        make,
        "building libkrunfw from third_party/upstream/libkrunfw",
    )?;

    let built = find_built_libkrunfw(&libkrunfw_root)?;
    let lib_dir = runtime_dir.join("lib");
    fs::create_dir_all(&lib_dir).with_context(|| format!("creating {}", lib_dir.display()))?;
    let target = lib_dir.join(LINUX_X86_64_LIBKRUNFW_NAME);
    fs::copy(&built, &target).with_context(|| {
        format!(
            "copying {} into runtime bundle as {}",
            built.display(),
            target.display()
        )
    })?;
    Ok(())
}

fn find_built_libkrunfw(libkrunfw_root: &Path) -> anyhow::Result<PathBuf> {
    let mut candidates = Vec::new();
    for entry in fs::read_dir(libkrunfw_root)
        .with_context(|| format!("reading {}", libkrunfw_root.display()))?
    {
        let path = entry?.path();
        let name = path.file_name().and_then(OsStr::to_str).unwrap_or_default();
        if path.is_file() && name.starts_with("libkrunfw") && name.contains(".so.") {
            candidates.push(path);
        }
    }
    candidates.sort();
    let built = candidates
        .into_iter()
        .last()
        .context("unable to locate built libkrunfw artifact")?;
    ensure!(
        built.is_file(),
        "missing built libkrunfw: {}",
        built.display()
    );
    Ok(built)
}
