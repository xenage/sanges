#[path = "runtime/source_build.rs"]
mod source_build;
#[path = "runtime/submodules.rs"]
mod submodules;

use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, ensure};

use super::cargo_ops::{cargo_build, run};
use super::types::{
    GUEST_AGENT_MANIFEST, Platform, PlatformOs, Profile, ResolvedArtifacts, RuntimeBundle,
    RuntimeBundleSource, absolutize, target_root,
};
use source_build::{
    macos_cc_linux_value, preferred_host_clang, preferred_ld_lld, preferred_libclang_dir,
    prepend_env_path, stage_libclang_runtime,
};
use submodules::ensure_upstream_checkout;

const GUEST_OUTPUT_DIR_ENV: &str = "SAGENS_GUEST_OUTPUT_DIR";
const GUEST_WORK_DIR_ENV: &str = "SAGENS_GUEST_WORK_DIR";
const RUNTIME_BUNDLE_DIR_ENV: &str = "SAGENS_RUNTIME_BUNDLE_DIR";

pub(super) fn maybe_init_submodules(root: &Path) -> anyhow::Result<()> {
    submodules::maybe_init_submodules(root)
}

pub(super) fn clean_runtime_dir(root: &Path, platform: Platform) -> anyhow::Result<()> {
    let runtime_dir = runtime_bundle_dir(root, platform);
    if runtime_dir.exists() {
        fs::remove_dir_all(&runtime_dir)
            .with_context(|| format!("removing {}", runtime_dir.display()))?;
    }
    Ok(())
}

pub(super) fn ensure_runtime_bundle(
    root: &Path,
    platform: Platform,
) -> anyhow::Result<RuntimeBundle> {
    let runtime_dir = runtime_bundle_dir(root, platform);
    let lib_dir = runtime_dir.join("lib");
    let share_dir = runtime_dir.join("share").join("krunkit");
    let libkrun = lib_dir.join(platform.lib_name());
    let source = if libkrun.is_file() {
        RuntimeBundleSource::Prebuilt
    } else {
        fs::create_dir_all(&lib_dir).with_context(|| format!("creating {}", lib_dir.display()))?;
        if platform.os == PlatformOs::Macos {
            fs::create_dir_all(&share_dir)
                .with_context(|| format!("creating {}", share_dir.display()))?;
        }
        build_libkrun_from_source(root, platform, &runtime_dir)?;
        RuntimeBundleSource::SourceBuild
    };
    let firmware = match platform.os {
        PlatformOs::Macos => {
            let path = share_dir.join("KRUN_EFI.silent.fd");
            ensure!(
                path.is_file(),
                "missing runtime firmware: {}",
                path.display()
            );
            Some(path)
        }
        PlatformOs::Linux => None,
    };
    ensure_platform_runtime_support(platform, &lib_dir)?;
    let runtime_support = collect_runtime_support(&lib_dir, &libkrun)?;
    Ok(RuntimeBundle {
        libkrun,
        firmware,
        runtime_support,
        source,
    })
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
    run(command, "building Alpine guest artifacts")
}

pub(super) fn resolve_artifacts(
    root: &Path,
    platform: Platform,
    runtime: RuntimeBundle,
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
    ensure!(
        runtime.libkrun.is_file(),
        "missing runtime library: {}",
        runtime.libkrun.display()
    );
    if let Some(firmware) = &runtime.firmware {
        ensure!(
            firmware.is_file(),
            "missing firmware: {}",
            firmware.display()
        );
    }
    Ok(ResolvedArtifacts {
        libkrun: runtime.libkrun,
        kernel,
        rootfs,
        firmware: runtime.firmware,
        runtime_support: runtime.runtime_support,
    })
}

pub(super) fn guest_kernel_path(root: &Path, platform: Platform) -> PathBuf {
    let kernel_name = match platform.os {
        PlatformOs::Macos => "vmlinuz-virt.pe.gz",
        PlatformOs::Linux => "vmlinuz-virt",
    };
    guest_output_dir(root, platform).join(kernel_name)
}

pub(super) fn guest_rootfs_path(root: &Path, platform: Platform) -> PathBuf {
    guest_output_dir(root, platform).join("rootfs.raw")
}

fn ensure_platform_runtime_support(platform: Platform, lib_dir: &Path) -> anyhow::Result<()> {
    if platform.os == PlatformOs::Macos {
        ensure_macos_libkrunfw(lib_dir)?;
    }
    Ok(())
}

fn ensure_macos_libkrunfw(lib_dir: &Path) -> anyhow::Result<()> {
    let target = lib_dir.join("libkrunfw.dylib");
    if target.is_file() {
        return Ok(());
    }
    let source = find_macos_libkrunfw().context(
        "missing libkrunfw.dylib for macOS runtime bundle; install a prebuilt libkrunfw (for example via Homebrew) or provide it in third_party/runtime/<platform>/lib",
    )?;
    fs::copy(&source, &target).with_context(|| {
        format!(
            "copying libkrunfw runtime support {} into {}",
            source.display(),
            target.display()
        )
    })?;
    Ok(())
}

fn find_macos_libkrunfw() -> Option<PathBuf> {
    [
        PathBuf::from("/opt/homebrew/lib/libkrunfw.dylib"),
        PathBuf::from("/usr/local/lib/libkrunfw.dylib"),
        PathBuf::from("/opt/homebrew/lib/libkrunfw.5.dylib"),
        PathBuf::from("/usr/local/lib/libkrunfw.5.dylib"),
    ]
    .into_iter()
    .find(|path| path.is_file())
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

fn runtime_bundle_dir(root: &Path, platform: Platform) -> PathBuf {
    env::var_os(RUNTIME_BUNDLE_DIR_ENV)
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .map(|path| absolutize(root, &path))
        .unwrap_or_else(|| {
            root.join("third_party")
                .join("runtime")
                .join(platform.as_str())
        })
}

fn build_libkrun_from_source(
    root: &Path,
    platform: Platform,
    runtime_dir: &Path,
) -> anyhow::Result<()> {
    let libkrun_root = ensure_upstream_checkout(root, "third_party/upstream/libkrun", "Makefile")?;
    let mut make = crate::cmd::tool_command("make");
    make.arg("-C").arg(&libkrun_root);
    match platform.os {
        PlatformOs::Macos => make.arg("EFI=1"),
        PlatformOs::Linux => make.arg("BLK=1"),
    };
    if platform.os == PlatformOs::Macos {
        let clang = preferred_host_clang().context(
            "missing clang for libkrun source build on macOS; install Xcode command line tools or Homebrew llvm",
        )?;
        let lld = preferred_ld_lld().context(
            "missing ld.lld for libkrun source build on macOS; install Homebrew lld or use a standard Rust toolchain",
        )?;
        let libclang_dir = preferred_libclang_dir(&clang).context(
            "missing libclang.dylib for libkrun source build on macOS; install Xcode command line tools or Homebrew llvm",
        )?;
        stage_libclang_runtime(&libkrun_root, &libclang_dir)?;
        prepend_env_path(
            &mut make,
            "PATH",
            lld.parent().context("ld.lld has no parent directory")?,
        );
        prepend_env_path(&mut make, "DYLD_LIBRARY_PATH", &libclang_dir);
        prepend_env_path(&mut make, "DYLD_FALLBACK_LIBRARY_PATH", &libclang_dir);
        make.env("LIBCLANG_PATH", &libclang_dir);
        make.arg(format!("CLANG={}", clang.display()));
        make.arg(format!(
            "CC_LINUX={}",
            macos_cc_linux_value(&libkrun_root, platform, &clang)
        ));
    }
    run(make, "building libkrun from third_party/upstream/libkrun")?;
    let built = find_built_lib(&libkrun_root.join("target").join("release"), platform)?;
    let lib_dir = runtime_dir.join("lib");
    fs::create_dir_all(&lib_dir).with_context(|| format!("creating {}", lib_dir.display()))?;
    fs::copy(&built, lib_dir.join(platform.lib_name()))
        .with_context(|| format!("copying {} into runtime bundle", built.display()))?;
    if platform.os == PlatformOs::Macos {
        let firmware_src = libkrun_root.join("edk2").join("KRUN_EFI.silent.fd");
        let firmware_dst = runtime_dir
            .join("share")
            .join("krunkit")
            .join("KRUN_EFI.silent.fd");
        ensure!(
            firmware_src.is_file(),
            "missing libkrun firmware source: {}",
            firmware_src.display()
        );
        fs::copy(&firmware_src, &firmware_dst).with_context(|| {
            format!(
                "copying firmware {} into runtime bundle",
                firmware_src.display()
            )
        })?;
    }
    Ok(())
}

fn find_built_lib(target_dir: &Path, platform: Platform) -> anyhow::Result<PathBuf> {
    let mut candidates = Vec::new();
    for entry in
        fs::read_dir(target_dir).with_context(|| format!("reading {}", target_dir.display()))?
    {
        let path = entry?.path();
        let name = path.file_name().and_then(OsStr::to_str).unwrap_or_default();
        let matches = match platform.os {
            PlatformOs::Macos => name.starts_with("libkrun") && name.ends_with(".dylib"),
            PlatformOs::Linux => name.starts_with("libkrun") && name.contains(".so"),
        };
        if matches && path.is_file() {
            candidates.push(path);
        }
    }
    candidates.sort();
    candidates
        .into_iter()
        .last()
        .context("unable to locate built libkrun artifact")
}

fn collect_runtime_support(lib_dir: &Path, libkrun: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(lib_dir).with_context(|| format!("reading {}", lib_dir.display()))? {
        let path = entry?.path();
        if !path.is_file() || path == libkrun {
            continue;
        }
        files.push(path);
    }
    files.sort();
    Ok(files)
}
