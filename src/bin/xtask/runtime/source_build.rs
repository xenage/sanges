use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, ensure};

use super::super::types::{Platform, PlatformArch, PlatformOs};

pub(super) struct SourcePatchGuards {
    _guards: Vec<SourcePatchGuard>,
}

struct SourcePatchGuard {
    path: PathBuf,
    original: String,
}

impl Drop for SourcePatchGuard {
    fn drop(&mut self) {
        let _ = fs::write(&self.path, &self.original);
    }
}

pub(super) fn patch_libkrun_sources(
    libkrun_root: &Path,
    platform: Platform,
) -> anyhow::Result<SourcePatchGuards> {
    let mut guards = Vec::new();
    if matches!(
        platform,
        Platform {
            os: PlatformOs::Macos,
            arch: PlatformArch::X86_64,
        }
    ) {
        if let Some(guard) = patch_libkrun_worker_message(libkrun_root)? {
            guards.push(guard);
        }
        if let Some(guard) = patch_libkrun_arch_x86_64_modules(libkrun_root)? {
            guards.push(guard);
        }
    }
    Ok(SourcePatchGuards { _guards: guards })
}

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

pub(super) fn prebuilt_runtime_bundle_ready(
    lib_dir: &Path,
    libkrun: &Path,
    platform: Platform,
) -> anyhow::Result<bool> {
    if !libkrun.is_file() {
        return Ok(false);
    }
    if platform.os != PlatformOs::Macos {
        return Ok(true);
    }
    runtime_support_matches_platform(lib_dir, platform)
}

fn runtime_support_matches_platform(lib_dir: &Path, platform: Platform) -> anyhow::Result<bool> {
    for entry in fs::read_dir(lib_dir).with_context(|| format!("reading {}", lib_dir.display()))? {
        let path = entry?.path();
        if !path.is_file() {
            continue;
        }
        let is_dylib = path
            .extension()
            .and_then(OsStr::to_str)
            .is_some_and(|extension| extension == "dylib");
        if is_dylib && !macos_binary_has_platform_arch(&path, platform.arch)? {
            return Ok(false);
        }
    }
    Ok(true)
}

#[cfg(target_os = "macos")]
fn macos_binary_has_platform_arch(path: &Path, arch: PlatformArch) -> anyhow::Result<bool> {
    let expected = match arch {
        PlatformArch::Aarch64 => "arm64",
        PlatformArch::X86_64 => "x86_64",
    };
    let output = Command::new("lipo")
        .arg("-archs")
        .arg(path)
        .output()
        .with_context(|| format!("running lipo on {}", path.display()))?;
    if !output.status.success() {
        return Ok(false);
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .any(|value| value == expected))
}

#[cfg(not(target_os = "macos"))]
fn macos_binary_has_platform_arch(_: &Path, _: PlatformArch) -> anyhow::Result<bool> {
    Ok(true)
}

fn rustup_which(tool: &str) -> Option<PathBuf> {
    let output = crate::cmd::tool_command("rustup")
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
    let output = crate::cmd::tool_command("xcrun")
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

fn patch_libkrun_worker_message(libkrun_root: &Path) -> anyhow::Result<Option<SourcePatchGuard>> {
    patch_libkrun_file(
        libkrun_root,
        ["src", "utils", "src", "worker_message.rs"],
        &[(
            "#[cfg(target_arch = \"x86_64\")]",
            "#[cfg(all(target_os = \"linux\", target_arch = \"x86_64\"))]",
        )],
    )
}

fn patch_libkrun_arch_x86_64_modules(
    libkrun_root: &Path,
) -> anyhow::Result<Option<SourcePatchGuard>> {
    patch_libkrun_file(
        libkrun_root,
        ["src", "arch", "src", "x86_64", "mod.rs"],
        &[
            ("mod gdt;", "#[cfg(target_os = \"linux\")]\nmod gdt;"),
            (
                "pub mod interrupts;",
                "#[cfg(target_os = \"linux\")]\npub mod interrupts;",
            ),
            (
                "pub mod msr;",
                "#[cfg(target_os = \"linux\")]\npub mod msr;",
            ),
            (
                "pub mod regs;",
                "#[cfg(target_os = \"linux\")]\npub mod regs;",
            ),
        ],
    )
}

fn patch_libkrun_file<const N: usize>(
    libkrun_root: &Path,
    relative_path: [&str; N],
    replacements: &[(&str, &str)],
) -> anyhow::Result<Option<SourcePatchGuard>> {
    let path = relative_path
        .into_iter()
        .fold(libkrun_root.to_path_buf(), |path, component| {
            path.join(component)
        });
    let original =
        fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let mut updated = original.clone();
    for (old, new) in replacements {
        updated = updated.replace(old, new);
    }
    if updated == original {
        return Ok(None);
    }
    fs::write(&path, updated).with_context(|| format!("writing {}", path.display()))?;
    Ok(Some(SourcePatchGuard { path, original }))
}
