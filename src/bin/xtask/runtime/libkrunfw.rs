use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::slice;

use anyhow::{Context, ensure};
use libloading::{Library, Symbol};

use super::super::cargo_ops::run;
use super::super::types::{Platform, PlatformArch, PlatformOs};
use super::submodules::ensure_upstream_checkout;

const LINUX_X86_64_KERNEL_NAME: &str = "libkrunfw-kernel.elf";

pub(super) fn ensure_linux_x86_64_embedded_kernel(
    root: &Path,
    runtime_dir: &Path,
) -> anyhow::Result<()> {
    let target = linux_x86_64_embedded_kernel_path(runtime_dir);
    if target.is_file() {
        return Ok(());
    }
    let libkrunfw_root =
        ensure_upstream_checkout(root, "third_party/upstream/libkrunfw", "Makefile")?;
    let mut make = crate::cmd::tool_command("make");
    make.arg("-C").arg(&libkrunfw_root);
    make.env_remove("CARGO_TARGET_DIR");
    run(
        make,
        "building libkrunfw from third_party/upstream/libkrunfw",
    )?;
    let built = find_built_linux_libkrunfw(&libkrunfw_root)?;
    let bytes = read_libkrunfw_kernel(&built)?;
    ensure!(
        bytes.starts_with(b"\x7fELF"),
        "expected libkrunfw embedded kernel to be ELF: {}",
        built.display()
    );
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    fs::write(&target, bytes).with_context(|| format!("writing {}", target.display()))?;
    Ok(())
}

pub(super) fn linux_x86_64_embedded_kernel_path(runtime_dir: &Path) -> PathBuf {
    runtime_dir
        .join("share")
        .join("krunkit")
        .join(LINUX_X86_64_KERNEL_NAME)
}

pub(super) fn packaged_kernel_path(platform: Platform, runtime_dir: &Path) -> Option<PathBuf> {
    if platform.os == PlatformOs::Linux && platform.arch == PlatformArch::X86_64 {
        let extracted = linux_x86_64_embedded_kernel_path(runtime_dir);
        if extracted.is_file() {
            return Some(extracted);
        }
    }
    None
}

fn find_built_linux_libkrunfw(root: &Path) -> anyhow::Result<PathBuf> {
    let mut candidates = Vec::new();
    for entry in fs::read_dir(root).with_context(|| format!("reading {}", root.display()))? {
        let path = entry?.path();
        let name = path.file_name().and_then(OsStr::to_str).unwrap_or_default();
        if path.is_file() && name.starts_with("libkrunfw.so.") {
            candidates.push(path);
        }
    }
    candidates.sort();
    candidates
        .into_iter()
        .last()
        .context("unable to locate built libkrunfw artifact")
}

fn read_libkrunfw_kernel(libkrunfw: &Path) -> anyhow::Result<Vec<u8>> {
    type KrunfwGetKernel =
        unsafe extern "C" fn(*mut u64, *mut u64, *mut usize) -> *mut std::ffi::c_char;

    let library = unsafe { Library::new(libkrunfw) }
        .with_context(|| format!("loading {}", libkrunfw.display()))?;
    let symbol: Symbol<'_, KrunfwGetKernel> =
        unsafe { library.get(b"krunfw_get_kernel\0") }.context("loading krunfw_get_kernel")?;
    let mut guest_addr = 0u64;
    let mut entry_addr = 0u64;
    let mut size = 0usize;
    let pointer = unsafe { symbol(&mut guest_addr, &mut entry_addr, &mut size) };
    ensure!(
        !pointer.is_null() && size > 0,
        "libkrunfw returned an empty kernel bundle"
    );
    ensure!(
        guest_addr != 0 && entry_addr != 0,
        "libkrunfw returned invalid kernel addresses"
    );
    Ok(unsafe { slice::from_raw_parts(pointer.cast::<u8>(), size) }.to_vec())
}
