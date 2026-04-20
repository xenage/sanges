use std::collections::BTreeSet;
use std::env;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, ensure};

use super::source_build::preferred_host_clang;

pub(super) fn build_macos_x86_64_hvf_runtime(
    root: &Path,
    runtime_dir: &Path,
) -> anyhow::Result<()> {
    let lib_dir = runtime_dir.join("lib");
    let share_dir = runtime_dir.join("share").join("krunkit");
    fs::create_dir_all(&lib_dir).with_context(|| format!("creating {}", lib_dir.display()))?;
    fs::create_dir_all(&share_dir).with_context(|| format!("creating {}", share_dir.display()))?;
    copy_macos_runtime_firmware(root, &share_dir)?;
    create_macos_runtime_marker(&lib_dir.join("libkrun.dylib"))?;
    bundle_macos_qemu_runtime(&lib_dir)?;
    Ok(())
}

pub(super) fn copy_optional_macos_libkrunfw(lib_dir: &Path) -> anyhow::Result<()> {
    let target = lib_dir.join("libkrunfw.dylib");
    if target.is_file() {
        return Ok(());
    }
    let Some(source) = [
        "/opt/homebrew/lib/libkrunfw.dylib",
        "/usr/local/lib/libkrunfw.dylib",
        "/opt/homebrew/lib/libkrunfw.5.dylib",
        "/usr/local/lib/libkrunfw.5.dylib",
    ]
    .into_iter()
    .map(PathBuf::from)
    .find(|path| path.is_file()) else {
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
    let sdk =
        macos_sdk_path().context("missing macOS SDK path for macOS x86_64 runtime marker build")?;
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
        .arg("-isysroot")
        .arg(&sdk)
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

fn macos_sdk_path() -> Option<PathBuf> {
    let output = Command::new("xcrun")
        .arg("--sdk")
        .arg("macosx")
        .arg("--show-sdk-path")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8(output.stdout).ok()?;
    let candidate = PathBuf::from(path.trim());
    candidate.is_dir().then_some(candidate)
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
        let resolved = fs::canonicalize(&dependency).unwrap_or(dependency);
        if !resolved.is_file() || !dependencies.insert(resolved.clone()) {
            continue;
        }
        collect_macos_binary_deps(&resolved, dependencies)?;
    }
    Ok(())
}

fn copy_runtime_support_file(source: &Path, target: &Path) -> anyhow::Result<()> {
    if target.exists() {
        fs::remove_file(target).with_context(|| format!("removing {}", target.display()))?;
    }
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
