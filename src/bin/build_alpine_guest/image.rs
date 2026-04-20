use std::env;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, bail, ensure};

pub(super) fn build_partitioned_ext4_image(
    rootfs_dir: &Path,
    image_path: &Path,
    min_image_mib: u64,
) -> anyhow::Result<()> {
    if let Some(parent) = image_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let size_mib = min_image_mib.max(estimate_image_size(rootfs_dir)?);
    let image =
        File::create(image_path).with_context(|| format!("creating {}", image_path.display()))?;
    image.set_len(size_mib * 1024 * 1024)?;
    drop(image);
    write_mbr_partition_table(image_path, size_mib)?;

    let mke2fs = find_mke2fs()?;
    let offset_bytes = 1024 * 1024_u64;
    let filesystem_mib = 64_u64.max(size_mib.saturating_sub(offset_bytes.div_ceil(1024 * 1024)));
    let status = Command::new(&mke2fs)
        .arg("-d")
        .arg(rootfs_dir)
        .arg("-t")
        .arg("ext4")
        .arg("-L")
        .arg("agent-rootfs")
        .arg("-F")
        .arg("-E")
        .arg(format!("offset={offset_bytes}"))
        .arg(image_path)
        .arg(format!("{filesystem_mib}M"))
        .status()
        .with_context(|| format!("running {}", mke2fs.display()))?;
    ensure!(
        status.success(),
        "{} exited with {status}",
        mke2fs.display()
    );
    Ok(())
}

pub(super) fn unpack_with_tar(archive_path: &Path, destination: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(destination)?;
    let tar = find_in_path("tar").context("tar is required to unpack Alpine archives")?;
    let status = Command::new(&tar)
        .arg("-x")
        .arg("-z")
        .arg("-f")
        .arg(archive_path)
        .arg("-C")
        .arg(destination)
        .status()
        .with_context(|| {
            format!(
                "running {} to unpack {}",
                tar.display(),
                archive_path.display()
            )
        })?;
    ensure!(
        status.success(),
        "failed to unpack {} with {}",
        archive_path.display(),
        tar.display()
    );
    Ok(())
}

fn write_mbr_partition_table(image_path: &Path, size_mib: u64) -> anyhow::Result<()> {
    let total_sectors = size_mib * 1024 * 1024 / 512;
    let start_lba = 2048_u64;
    let partition_sectors = total_sectors
        .checked_sub(start_lba)
        .context("rootfs image is too small for a partitioned layout")?;
    let mut mbr = [0_u8; 512];
    let mut entry = [0_u8; 16];
    entry[1..4].copy_from_slice(&[0x00, 0x02, 0x00]);
    entry[4] = 0x83;
    entry[5..8].copy_from_slice(&[0xff, 0xff, 0xff]);
    entry[8..12].copy_from_slice(&(start_lba as u32).to_le_bytes());
    entry[12..16].copy_from_slice(&(partition_sectors as u32).to_le_bytes());
    mbr[446..462].copy_from_slice(&entry);
    mbr[510..512].copy_from_slice(&[0x55, 0xaa]);

    let mut file = File::options()
        .write(true)
        .open(image_path)
        .with_context(|| format!("opening {} for MBR write", image_path.display()))?;
    file.write_all(&mbr)?;
    Ok(())
}

fn estimate_image_size(rootfs_dir: &Path) -> anyhow::Result<u64> {
    let mut total = 0_u64;
    for entry in walkdir(rootfs_dir)? {
        let metadata = fs::symlink_metadata(&entry)?;
        if metadata.file_type().is_symlink() {
            total += fs::read_link(&entry)?.as_os_str().len() as u64;
        } else if metadata.is_file() {
            total += metadata.len();
        }
    }
    let padded = ((total as f64 * 1.4) / (1024.0 * 1024.0)).ceil() as u64;
    Ok(128_u64.max(padded + 32))
}

fn walkdir(root: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut entries = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(path) = stack.pop() {
        for child in fs::read_dir(&path).with_context(|| format!("reading {}", path.display()))? {
            let child = child?.path();
            let file_type = fs::symlink_metadata(&child)?.file_type();
            if file_type.is_dir() {
                stack.push(child.clone());
            }
            entries.push(child);
        }
    }
    Ok(entries)
}

fn find_mke2fs() -> anyhow::Result<PathBuf> {
    let candidates = [
        find_in_path("mke2fs"),
        find_in_path("mkfs.ext4"),
        Some(PathBuf::from("/opt/homebrew/opt/e2fsprogs/sbin/mke2fs")),
        Some(PathBuf::from("/opt/homebrew/opt/e2fsprogs/sbin/mkfs.ext4")),
    ];
    for candidate in candidates.into_iter().flatten() {
        if candidate.exists() {
            return Ok(candidate);
        }
    }
    bail!("mke2fs or mkfs.ext4 is required to build the Alpine guest image")
}

fn find_in_path(binary: &str) -> Option<PathBuf> {
    env::var_os("PATH").and_then(|paths| {
        env::split_paths(&paths)
            .map(|dir| dir.join(binary))
            .find(|candidate| candidate.exists())
    })
}
