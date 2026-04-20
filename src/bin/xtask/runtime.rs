#[path = "runtime/source_build.rs"]
mod source_build;
#[path = "runtime/submodules.rs"]
mod submodules;

use std::collections::BTreeSet;
use std::env;
use std::ffi::OsStr;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, ensure};

use super::cargo_ops::{cargo_build, run};
use super::types::{
    GUEST_AGENT_MANIFEST, Platform, PlatformArch, PlatformOs, Profile, ResolvedArtifacts,
    RuntimeBundle, RuntimeBundleSource, absolutize, target_root,
};
use source_build::{
    macos_cc_linux_value, patch_libkrun_sources, prebuilt_runtime_bundle_ready,
    preferred_host_clang, preferred_ld_lld, preferred_libclang_dir, prepend_env_path,
    stage_libclang_runtime,
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
    let source = if prebuilt_runtime_bundle_ready(&lib_dir, &libkrun, platform)? {
        RuntimeBundleSource::Prebuilt
    } else {
        if runtime_dir.exists() {
            fs::remove_dir_all(&runtime_dir)
                .with_context(|| format!("removing {}", runtime_dir.display()))?;
        }
        fs::create_dir_all(&lib_dir).with_context(|| format!("creating {}", lib_dir.display()))?;
        if platform.os == PlatformOs::Macos {
            fs::create_dir_all(&share_dir)
                .with_context(|| format!("creating {}", share_dir.display()))?;
        }
        build_runtime_bundle_from_source(root, platform, &runtime_dir)?;
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
        PlatformOs::Macos if platform.arch == PlatformArch::X86_64 => "vmlinuz-virt",
        PlatformOs::Macos => "vmlinuz-virt.pe.gz",
        PlatformOs::Linux => "vmlinuz-virt",
    };
    guest_output_dir(root, platform).join(kernel_name)
}

pub(super) fn guest_rootfs_path(root: &Path, platform: Platform) -> PathBuf {
    guest_output_dir(root, platform).join("rootfs.raw")
}

fn ensure_platform_runtime_support(platform: Platform, lib_dir: &Path) -> anyhow::Result<()> {
    if platform.os == PlatformOs::Macos && platform.arch == PlatformArch::Aarch64 {
        copy_optional_macos_libkrunfw(lib_dir)?;
    }
    Ok(())
}

fn copy_optional_macos_libkrunfw(lib_dir: &Path) -> anyhow::Result<()> {
    let target = lib_dir.join("libkrunfw.dylib");
    if target.is_file() {
        return Ok(());
    }
    let Some(source) = find_macos_libkrunfw() else {
        return Ok(());
    };
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

fn build_runtime_bundle_from_source(
    root: &Path,
    platform: Platform,
    runtime_dir: &Path,
) -> anyhow::Result<()> {
    if platform.os == PlatformOs::Macos && platform.arch == PlatformArch::X86_64 {
        return build_macos_x86_64_hvf_runtime(root, runtime_dir);
    }
    build_libkrun_from_source(root, platform, runtime_dir)
}

fn build_libkrun_from_source(
    root: &Path,
    platform: Platform,
    runtime_dir: &Path,
) -> anyhow::Result<()> {
    let libkrun_root = ensure_upstream_checkout(root, "third_party/upstream/libkrun", "Makefile")?;
    let _source_patches = patch_libkrun_sources(&libkrun_root, platform)?;
    let mut make = crate::cmd::tool_command("make");
    make.arg("-C").arg(&libkrun_root);
    // build-local.sh points Cargo at a shared temp target dir, but the upstream
    // Makefile expects artifacts under its local ./target tree.
    make.env_remove("CARGO_TARGET_DIR");
    match platform.os {
        PlatformOs::Macos => make.arg("EFI=1"),
        PlatformOs::Linux => make.arg("BLK=1"),
    };
    if platform.os == PlatformOs::Macos {
        if let Some(arch) = upstream_libkrun_arch(platform) {
            make.arg(format!("ARCH={arch}"));
        }
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

fn build_macos_x86_64_hvf_runtime(root: &Path, runtime_dir: &Path) -> anyhow::Result<()> {
    let lib_dir = runtime_dir.join("lib");
    let share_dir = runtime_dir.join("share").join("krunkit");
    fs::create_dir_all(&lib_dir).with_context(|| format!("creating {}", lib_dir.display()))?;
    fs::create_dir_all(&share_dir).with_context(|| format!("creating {}", share_dir.display()))?;
    copy_macos_runtime_firmware(root, &share_dir)?;
    create_macos_runtime_marker(&lib_dir.join("libkrun.dylib"))?;
    bundle_macos_qemu_runtime(&lib_dir)?;
    Ok(())
}

fn copy_macos_runtime_firmware(root: &Path, share_dir: &Path) -> anyhow::Result<()> {
    let target = share_dir.join("KRUN_EFI.silent.fd");
    if target.is_file() {
        return Ok(());
    }
    let source = [
        root.join("third_party/runtime/macos-x86_64/share/krunkit/KRUN_EFI.silent.fd"),
        root.join("third_party/upstream/libkrun/edk2/KRUN_EFI.silent.fd"),
        root.join("third_party/upstream/krunkit/edk2/KRUN_EFI.silent.fd"),
    ]
    .into_iter()
    .find(|path| path.is_file())
    .context("missing macOS runtime firmware source for macos-x86_64")?;
    fs::copy(&source, &target).with_context(|| {
        format!(
            "copying macOS runtime firmware {} into {}",
            source.display(),
            target.display()
        )
    })?;
    Ok(())
}

fn create_macos_runtime_marker(target: &Path) -> anyhow::Result<()> {
    let clang = preferred_host_clang().context(
        "missing clang for macOS x86_64 runtime marker build; install Xcode command line tools",
    )?;
    let source = target.with_extension("c");
    fs::write(
        &source,
        "int sagens_hvf_runtime_marker(void) { return 0; }\n",
    )
    .with_context(|| format!("writing {}", source.display()))?;
    let status = Command::new(&clang)
        .arg("-dynamiclib")
        .arg("-arch")
        .arg("x86_64")
        .arg("-Wl,-install_name,@rpath/libkrun.dylib")
        .arg("-o")
        .arg(target)
        .arg(&source)
        .status()
        .with_context(|| format!("building {}", target.display()))?;
    let _ = fs::remove_file(&source);
    ensure!(
        status.success(),
        "clang failed to build {}",
        target.display()
    );
    Ok(())
}

fn bundle_macos_qemu_runtime(lib_dir: &Path) -> anyhow::Result<()> {
    let qemu = find_macos_qemu_binary().context(
        "missing qemu-system-x86_64 on PATH; install Homebrew qemu for macos-x86_64 runtime builds",
    )?;
    ensure_macos_binary_arch(&qemu, "x86_64")
        .with_context(|| format!("validating {}", qemu.display()))?;
    let target = lib_dir.join("qemu-system-x86_64");
    copy_runtime_support_file(&qemu, &target)?;
    let mut dependencies = BTreeSet::new();
    collect_macos_binary_deps(&qemu, &mut dependencies)?;
    for dependency in dependencies {
        let destination = lib_dir.join(
            dependency
                .file_name()
                .context("dependency path has no file name")?,
        );
        copy_runtime_support_file(&dependency, &destination)?;
    }
    Ok(())
}

fn ensure_macos_binary_arch(path: &Path, arch: &str) -> anyhow::Result<()> {
    let output = Command::new("lipo")
        .arg("-archs")
        .arg(path)
        .output()
        .with_context(|| format!("running lipo on {}", path.display()))?;
    ensure!(
        output.status.success(),
        "lipo -archs failed for {}",
        path.display()
    );
    let archs = String::from_utf8(output.stdout).context("decoding lipo output")?;
    ensure!(
        archs.split_whitespace().any(|value| value == arch),
        "{} does not contain required architecture {}",
        path.display(),
        arch
    );
    Ok(())
}

fn find_macos_qemu_binary() -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    env::split_paths(&path)
        .map(|dir| dir.join("qemu-system-x86_64"))
        .find(|candidate| candidate.is_file())
}

fn collect_macos_binary_deps(
    root: &Path,
    dependencies: &mut BTreeSet<PathBuf>,
) -> anyhow::Result<()> {
    let output = Command::new("otool")
        .arg("-L")
        .arg(root)
        .output()
        .with_context(|| format!("running otool on {}", root.display()))?;
    ensure!(
        output.status.success(),
        "otool -L failed for {}",
        root.display()
    );
    let stdout = String::from_utf8(output.stdout).context("decoding otool output")?;
    for line in stdout.lines().skip(1) {
        let path = line.trim();
        if path.is_empty() {
            continue;
        }
        let Some((library, _)) = path.split_once(" (compatibility version") else {
            continue;
        };
        if library.starts_with("/usr/lib/") || library.starts_with("/System/Library/") {
            continue;
        }
        if !library.starts_with('/') {
            continue;
        }
        let dependency = PathBuf::from(library);
        if !dependency.is_file() || !dependencies.insert(dependency.clone()) {
            continue;
        }
        collect_macos_binary_deps(&dependency, dependencies)?;
    }
    Ok(())
}

fn copy_runtime_support_file(source: &Path, target: &Path) -> anyhow::Result<()> {
    fs::copy(source, target).with_context(|| {
        format!(
            "copying runtime support {} into {}",
            source.display(),
            target.display()
        )
    })?;
    #[cfg(unix)]
    {
        let mode = fs::metadata(source)
            .with_context(|| format!("reading {}", source.display()))?
            .permissions()
            .mode();
        fs::set_permissions(target, fs::Permissions::from_mode(mode))
            .with_context(|| format!("setting mode on {}", target.display()))?;
    }
    Ok(())
}

fn upstream_libkrun_arch(platform: Platform) -> Option<&'static str> {
    if platform.os != PlatformOs::Macos {
        return None;
    }
    Some(match platform.arch {
        // Upstream Makefile uses ARCH in Debian and FreeBSD download URLs.
        // Their archive naming is amd64, while our host arch remains x86_64.
        PlatformArch::X86_64 => "amd64",
        PlatformArch::Aarch64 => "arm64",
    })
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
