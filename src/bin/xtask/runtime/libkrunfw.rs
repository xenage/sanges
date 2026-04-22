use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;

use anyhow::{Context, ensure};

use super::super::types::{Platform, PlatformArch, PlatformOs};
use super::submodules::ensure_upstream_checkout;

const LINUX_LIBKRUNFW_NAME: &str = "libkrunfw.so.5";
const MACOS_AARCH64_LIBKRUNFW_NAME: &str = "libkrunfw.5.dylib";

pub(super) fn maybe_build_libkrunfw_support(
    root: &Path,
    platform: Platform,
    runtime_dir: &Path,
) -> anyhow::Result<()> {
    match platform {
        Platform {
            os: PlatformOs::Linux,
            arch: PlatformArch::X86_64,
        } => build_linux_libkrunfw(root, runtime_dir, None),
        Platform {
            os: PlatformOs::Linux,
            arch: PlatformArch::Aarch64,
        } => build_linux_libkrunfw(root, runtime_dir, Some("arm64")),
        _ => Ok(()),
    }
}

pub(super) fn required_runtime_support_files(platform: Platform) -> &'static [&'static str] {
    match platform {
        Platform {
            os: PlatformOs::Linux,
            arch: PlatformArch::X86_64,
        }
        | Platform {
            os: PlatformOs::Linux,
            arch: PlatformArch::Aarch64,
        } => &[LINUX_LIBKRUNFW_NAME],
        _ => &[],
    }
}

pub(super) fn ensure_macos_aarch64_libkrunfw(root: &Path, lib_dir: &Path) -> anyhow::Result<()> {
    let target = lib_dir.join(MACOS_AARCH64_LIBKRUNFW_NAME);
    let legacy = lib_dir.join("libkrunfw.dylib");
    if target.is_file() {
        if legacy.is_file() {
            fs::remove_file(&legacy)
                .with_context(|| format!("removing duplicate {}", legacy.display()))?;
        }
        return Ok(());
    }
    if legacy.is_file() {
        fs::rename(&legacy, &target).with_context(|| {
            format!(
                "renaming bundled macOS libkrunfw {} to {}",
                legacy.display(),
                target.display()
            )
        })?;
        return Ok(());
    }

    let source = [
        root.join("third_party/runtime/macos-aarch64/lib/libkrunfw.5.dylib"),
        root.join("third_party/runtime/macos-aarch64/lib/libkrunfw.dylib"),
        PathBuf::from("/opt/homebrew/opt/libkrunfw/lib/libkrunfw.5.dylib"),
        PathBuf::from("/usr/local/opt/libkrunfw/lib/libkrunfw.5.dylib"),
        PathBuf::from("/opt/homebrew/lib/libkrunfw.5.dylib"),
        PathBuf::from("/usr/local/lib/libkrunfw.5.dylib"),
        PathBuf::from("/opt/homebrew/lib/libkrunfw.dylib"),
        PathBuf::from("/usr/local/lib/libkrunfw.dylib"),
    ]
    .into_iter()
    .find(|path| path.is_file())
    .context("missing macOS aarch64 libkrunfw runtime support")?;

    fs::copy(&source, &target).with_context(|| {
        format!(
            "copying macOS aarch64 libkrunfw {} into {}",
            source.display(),
            target.display()
        )
    })?;
    Ok(())
}

fn build_linux_libkrunfw(
    root: &Path,
    runtime_dir: &Path,
    arch: Option<&'static str>,
) -> anyhow::Result<()> {
    let libkrunfw_root =
        ensure_upstream_checkout(root, "third_party/upstream/libkrunfw", "Makefile")?;
    let mut make = crate::cmd::tool_command("make");
    make.arg("-C").arg(&libkrunfw_root);
    make.arg(format!("-j{}", host_parallelism()));
    make.arg("MAKEFLAGS=");
    if let Some(arch) = arch {
        make.arg(format!("ARCH={arch}"));
    }
    make.env_remove("CARGO_TARGET_DIR");
    super::super::cargo_ops::run(
        make,
        "building libkrunfw from third_party/upstream/libkrunfw",
    )?;

    let built = find_built_libkrunfw(&libkrunfw_root)?;
    let lib_dir = runtime_dir.join("lib");
    fs::create_dir_all(&lib_dir).with_context(|| format!("creating {}", lib_dir.display()))?;
    let target = lib_dir.join(LINUX_LIBKRUNFW_NAME);
    fs::copy(&built, &target).with_context(|| {
        format!(
            "copying {} into runtime bundle as {}",
            built.display(),
            target.display()
        )
    })?;
    Ok(())
}

fn host_parallelism() -> usize {
    thread::available_parallelism().map_or(1, usize::from)
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
