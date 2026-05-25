use std::fs::{self, File};
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use ext4_lwext4::{Ext4Fs, FileBlockDevice, MkfsOptions, OpenFlags, mkfs};

pub(super) fn build_ext4_image(
    rootfs_dir: &Path,
    image_path: &Path,
    min_image_mib: u64,
) -> anyhow::Result<()> {
    if let Some(parent) = image_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let size_mib = min_image_mib.max(estimate_image_size(rootfs_dir)?);
    let device = FileBlockDevice::create(image_path, size_mib * 1024 * 1024)
        .with_context(|| format!("creating ext4 image {}", image_path.display()))?;
    let options = MkfsOptions::ext4()
        .with_block_size(4096)
        .with_label("sagens-image");
    mkfs(device, &options).context("formatting ext4 image")?;
    let device = FileBlockDevice::open(image_path)
        .with_context(|| format!("opening ext4 image {}", image_path.display()))?;
    let fs = Ext4Fs::mount(device, false).context("mounting ext4 image")?;
    populate_ext4_from_dir(rootfs_dir, &fs)?;
    fs.umount().context("unmounting ext4 image")
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

fn populate_ext4_from_dir(rootfs_dir: &Path, ext4: &Ext4Fs) -> anyhow::Result<()> {
    let root_mode = fs::metadata(rootfs_dir)
        .with_context(|| format!("reading {}", rootfs_dir.display()))?
        .permissions()
        .mode()
        & 0o7777;
    ext4.set_permissions("/", root_mode)
        .context("setting ext4 root permissions")?;
    copy_directory_entries(rootfs_dir, Path::new(""), ext4)
}

fn copy_directory_entries(
    source_dir: &Path,
    relative_dir: &Path,
    ext4: &Ext4Fs,
) -> anyhow::Result<()> {
    let mut children = fs::read_dir(source_dir)
        .with_context(|| format!("reading {}", source_dir.display()))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .with_context(|| format!("iterating {}", source_dir.display()))?;
    children.sort_by_key(|entry| entry.file_name());
    for child in children {
        copy_directory_entry(&child.path(), &relative_dir.join(child.file_name()), ext4)?;
    }
    Ok(())
}

fn copy_directory_entry(
    host_path: &Path,
    relative_path: &Path,
    ext4: &Ext4Fs,
) -> anyhow::Result<()> {
    let metadata = fs::symlink_metadata(host_path)
        .with_context(|| format!("reading {}", host_path.display()))?;
    let image_path = ext4_path(relative_path)?;
    let mode = metadata.permissions().mode() & 0o7777;
    if metadata.file_type().is_dir() {
        ext4.mkdir(&image_path, mode)
            .with_context(|| format!("creating directory {image_path} in ext4 image"))?;
        ext4.set_permissions(&image_path, mode)
            .with_context(|| format!("setting directory permissions on {image_path}"))?;
        return copy_directory_entries(host_path, relative_path, ext4);
    }
    if metadata.file_type().is_symlink() {
        return copy_symlink(host_path, &image_path, ext4);
    }
    if metadata.is_file() {
        return copy_file(host_path, &image_path, mode, ext4);
    }
    bail!("unsupported rootfs entry type: {}", host_path.display());
}

fn copy_symlink(host_path: &Path, image_path: &str, ext4: &Ext4Fs) -> anyhow::Result<()> {
    let target = fs::read_link(host_path)
        .with_context(|| format!("reading symlink {}", host_path.display()))?;
    let target = target
        .to_str()
        .context("symlink target is not valid UTF-8")?;
    ext4.symlink(target, image_path)
        .with_context(|| format!("creating symlink {image_path} -> {target}"))
}

fn copy_file(host_path: &Path, image_path: &str, mode: u32, ext4: &Ext4Fs) -> anyhow::Result<()> {
    let mut input =
        File::open(host_path).with_context(|| format!("opening {}", host_path.display()))?;
    let mut output = ext4
        .open(image_path, OpenFlags::CREATE | OpenFlags::WRITE)
        .with_context(|| format!("creating file {image_path} in ext4 image"))?;
    io::copy(&mut input, &mut output)
        .with_context(|| format!("copying {} into ext4 image", host_path.display()))?;
    ext4.set_permissions(image_path, mode)
        .with_context(|| format!("setting file permissions on {image_path}"))
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

fn ext4_path(relative_path: &Path) -> anyhow::Result<String> {
    let path = relative_path
        .to_str()
        .context("rootfs path is not valid UTF-8")?
        .replace('\\', "/");
    Ok(format!("/{}", path))
}
