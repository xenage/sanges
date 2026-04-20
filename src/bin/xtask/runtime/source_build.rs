use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, ensure};

use super::super::types::{Platform, PlatformArch};

pub(super) fn preferred_host_clang() -> Option<PathBuf> {
    xcrun_find("clang").or_else(|| {
        let path = PathBuf::from("/usr/bin/clang");
        path.is_file().then_some(path)
    })
}

pub(super) fn preferred_zig() -> Option<PathBuf> {
    xcrun_find("zig").or_else(|| {
        let path = PathBuf::from("/opt/homebrew/bin/zig");
        path.is_file().then_some(path)
    })
}

pub(super) fn preferred_ld_lld() -> Option<PathBuf> {
    let rustc = rustup_which("rustc")?;
    let toolchain_root = rustc.parent()?.parent()?;
    let rustlib_dir = toolchain_root.join("lib").join("rustlib");
    let entries = fs::read_dir(rustlib_dir).ok()?;
    for entry in entries {
        let candidate = entry.ok()?.path().join("bin").join("gcc-ld").join("ld.lld");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

pub(super) fn preferred_libclang_dir(clang: &Path) -> Option<PathBuf> {
    let direct = clang.parent()?.parent()?.join("lib");
    if direct.join("libclang.dylib").is_file() {
        return Some(direct);
    }
    let frameworks = clang.parent()?.parent()?.parent()?.join("Frameworks");
    frameworks
        .join("libclang.dylib")
        .is_file()
        .then_some(frameworks)
}

pub(super) fn stage_libclang_runtime(
    libkrun_root: &Path,
    libclang_dir: &Path,
) -> anyhow::Result<()> {
    let target_root = libkrun_root.join("target");
    for file_name in ["libclang.dylib", "libLLVM.dylib"] {
        let source = libclang_dir.join(file_name);
        if file_name == "libclang.dylib" {
            ensure!(
                source.is_file(),
                "missing libclang runtime: {}",
                source.display()
            );
        }
        if !source.is_file() {
            continue;
        }
        for destination in [
            target_root.join("lib").join(file_name),
            target_root.join("release").join(file_name),
            target_root.join("release").join("deps").join(file_name),
        ] {
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("creating {}", parent.display()))?;
            }
            if destination.exists() {
                fs::remove_file(&destination)
                    .with_context(|| format!("removing {}", destination.display()))?;
            }
            fs::copy(&source, &destination).with_context(|| {
                format!(
                    "staging clang runtime {} to {}",
                    source.display(),
                    destination.display()
                )
            })?;
        }
    }
    Ok(())
}

pub(super) fn prepend_env_path(command: &mut Command, name: &str, prefix: &Path) {
    let mut value = prefix.as_os_str().to_os_string();
    if let Some(existing) = env::var_os(name).filter(|value| !value.is_empty()) {
        value.push(OsStr::new(":"));
        value.push(existing);
    }
    command.env(name, value);
}

pub(super) fn macos_cc_linux_value(
    libkrun_root: &Path,
    platform: Platform,
    clang: &Path,
) -> String {
    if let Some(zig) = preferred_zig() {
        let target = match platform.arch {
            PlatformArch::Aarch64 => "aarch64-linux-musl",
            PlatformArch::X86_64 => "x86_64-linux-musl",
        };
        return format!("{} cc -target {}", zig.display(), target);
    }
    let triplet = match platform.arch {
        PlatformArch::Aarch64 => "aarch64-linux-gnu",
        PlatformArch::X86_64 => "x86_64-linux-gnu",
    };
    let sysroot = libkrun_root.join("linux-sysroot");
    let gcc_lib_dir = sysroot.join("usr/lib/gcc").join(triplet).join("12");
    format!(
        "{} -target {} -fuse-ld=lld --sysroot {} -B{} -L{} -Wno-c23-extensions",
        clang.display(),
        triplet,
        sysroot.display(),
        gcc_lib_dir.display(),
        gcc_lib_dir.display(),
    )
}

fn rustup_which(tool: &str) -> Option<PathBuf> {
    let output = Command::new("rtk")
        .arg("rustup")
        .arg("which")
        .arg(tool)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8(output.stdout).ok()?;
    let candidate = PathBuf::from(path.trim());
    candidate.is_file().then_some(candidate)
}

fn xcrun_find(tool: &str) -> Option<PathBuf> {
    let output = Command::new("rtk")
        .arg("xcrun")
        .arg("--find")
        .arg(tool)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8(output.stdout).ok()?;
    let candidate = PathBuf::from(path.trim());
    candidate.is_file().then_some(candidate)
}
