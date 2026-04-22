use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;
use sha2::{Digest, Sha256};

use super::super::types::Platform;

const GUEST_ARTIFACT_FINGERPRINT_FILE: &str = ".build-fingerprint";

pub(crate) fn guest_artifacts_stale(root: &Path, platform: Platform) -> anyhow::Result<bool> {
    let kernel = super::guest_kernel_path(root, platform);
    let rootfs = super::guest_rootfs_path(root, platform);
    let fingerprint_path = guest_artifact_fingerprint_path(root, platform);
    if !kernel.is_file() || !rootfs.is_file() || !fingerprint_path.is_file() {
        return Ok(true);
    }
    let current = current_guest_artifact_fingerprint(root, platform)?;
    let cached = fs::read_to_string(&fingerprint_path)
        .with_context(|| format!("reading {}", fingerprint_path.display()))?;
    Ok(cached.trim() != current)
}

pub(crate) fn write_guest_artifact_fingerprint(
    root: &Path,
    platform: Platform,
) -> anyhow::Result<()> {
    let output_dir = super::guest_output_dir(root, platform);
    fs::create_dir_all(&output_dir)
        .with_context(|| format!("creating {}", output_dir.display()))?;
    let fingerprint_path = guest_artifact_fingerprint_path(root, platform);
    let fingerprint = current_guest_artifact_fingerprint(root, platform)?;
    fs::write(&fingerprint_path, format!("{fingerprint}\n"))
        .with_context(|| format!("writing {}", fingerprint_path.display()))
}

fn guest_artifact_fingerprint_path(root: &Path, platform: Platform) -> PathBuf {
    super::guest_output_dir(root, platform).join(GUEST_ARTIFACT_FINGERPRINT_FILE)
}

fn current_guest_artifact_fingerprint(root: &Path, platform: Platform) -> anyhow::Result<String> {
    let mut hasher = Sha256::new();
    hasher.update(platform.as_str().as_bytes());
    for relative in guest_artifact_fingerprint_inputs() {
        hash_path_recursively(root, &root.join(relative), &mut hasher)?;
    }
    let digest = hasher.finalize();
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(&mut encoded, "{byte:02x}");
    }
    Ok(encoded)
}

fn guest_artifact_fingerprint_inputs() -> &'static [&'static str] {
    &[
        "Cargo.lock",
        "src/bin/build-alpine-guest.rs",
        "src/bin/build_alpine_guest",
        "src/bin/xtask/runtime",
        "crates/sagens-guest-agent",
        "crates/sagens-guest-contract",
        "guest-agent",
        "shared",
    ]
}

fn hash_path_recursively(root: &Path, path: &Path, hasher: &mut Sha256) -> anyhow::Result<()> {
    let metadata =
        fs::symlink_metadata(path).with_context(|| format!("reading {}", path.display()))?;
    let relative = path.strip_prefix(root).unwrap_or(path);
    hasher.update(relative.to_string_lossy().as_bytes());
    if metadata.is_dir() {
        hasher.update(b"D");
        let mut children = fs::read_dir(path)
            .with_context(|| format!("reading {}", path.display()))?
            .collect::<Result<Vec<_>, _>>()
            .with_context(|| format!("iterating {}", path.display()))?;
        children.sort_by_key(|entry| entry.file_name());
        for child in children {
            hash_path_recursively(root, &child.path(), hasher)?;
        }
        return Ok(());
    }
    if metadata.file_type().is_symlink() {
        hasher.update(b"L");
        let target =
            fs::read_link(path).with_context(|| format!("reading symlink {}", path.display()))?;
        hasher.update(target.to_string_lossy().as_bytes());
        return Ok(());
    }
    if metadata.is_file() {
        hasher.update(b"F");
        hasher.update(fs::read(path).with_context(|| format!("reading {}", path.display()))?);
        return Ok(());
    }
    anyhow::bail!("unsupported fingerprint path type: {}", path.display());
}
