use std::fs::{self, File};
use std::io::{self, Seek, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use ext4_lwext4::{Ext4Fs, FileBlockDevice, MkfsOptions, OpenFlags, mkfs};
use flate2::read::MultiGzDecoder;
use tar::Archive;
use tempfile::{TempDir, tempdir_in};

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

    let offset_bytes = 1024 * 1024_u64;
    let filesystem_bytes =
        64_u64.max(size_mib.saturating_sub(offset_bytes.div_ceil(1024 * 1024))) * 1024 * 1024;
    let staging_dir = create_staging_dir(image_path)?;
    let staging_path = staging_dir.path().join("rootfs.ext4");
    build_staged_ext4_image(rootfs_dir, &staging_path, filesystem_bytes)?;
    copy_staged_partition(&staging_path, image_path, offset_bytes)?;
    Ok(())
}

pub(super) fn unpack_with_tar(archive_path: &Path, destination: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(destination)?;
    let archive =
        File::open(archive_path).with_context(|| format!("opening {}", archive_path.display()))?;
    let decoder = MultiGzDecoder::new(archive);
    let mut archive = Archive::new(decoder);
    archive.unpack(destination).with_context(|| {
        format!(
            "unpacking {} to {}",
            archive_path.display(),
            destination.display()
        )
    })?;
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

fn create_staging_dir(image_path: &Path) -> anyhow::Result<TempDir> {
    let parent = image_path.parent().unwrap_or_else(|| Path::new("."));
    tempdir_in(parent)
        .with_context(|| format!("creating staging directory under {}", parent.display()))
}

fn build_staged_ext4_image(
    rootfs_dir: &Path,
    staging_path: &Path,
    size_bytes: u64,
) -> anyhow::Result<()> {
    let device = FileBlockDevice::create(staging_path, size_bytes)
        .with_context(|| format!("creating staged ext4 image {}", staging_path.display()))?;
    let options = MkfsOptions::ext4()
        .with_block_size(4096)
        .with_label("agent-rootfs");
    mkfs(device, &options).context("formatting staged ext4 image")?;

    let device = FileBlockDevice::open(staging_path)
        .with_context(|| format!("opening staged ext4 image {}", staging_path.display()))?;
    let fs = Ext4Fs::mount(device, false).context("mounting staged ext4 image")?;
    populate_ext4_from_dir(rootfs_dir, &fs)?;
    fs.umount().context("unmounting staged ext4 image")
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
        let host_path = child.path();
        let relative_path = relative_dir.join(child.file_name());
        let metadata = fs::symlink_metadata(&host_path)
            .with_context(|| format!("reading {}", host_path.display()))?;
        let image_path = ext4_path(&relative_path)?;
        let mode = metadata.permissions().mode() & 0o7777;
        if metadata.file_type().is_dir() {
            ext4.mkdir(&image_path, mode)
                .with_context(|| format!("creating directory {image_path} in staged ext4 image"))?;
            ext4.set_permissions(&image_path, mode)
                .with_context(|| format!("setting directory permissions on {image_path}"))?;
            copy_directory_entries(&host_path, &relative_path, ext4)?;
            continue;
        }
        if metadata.file_type().is_symlink() {
            let target = fs::read_link(&host_path)
                .with_context(|| format!("reading symlink {}", host_path.display()))?;
            let target = target
                .to_str()
                .context("symlink target is not valid UTF-8")?;
            ext4.symlink(target, &image_path)
                .with_context(|| format!("creating symlink {image_path} -> {target}"))?;
            continue;
        }
        if metadata.is_file() {
            let mut input = File::open(&host_path)
                .with_context(|| format!("opening {}", host_path.display()))?;
            let mut output = ext4
                .open(&image_path, OpenFlags::CREATE | OpenFlags::WRITE)
                .with_context(|| format!("creating file {image_path} in staged ext4 image"))?;
            io::copy(&mut input, &mut output).with_context(|| {
                format!("copying {} into staged ext4 image", host_path.display())
            })?;
            ext4.set_permissions(&image_path, mode)
                .with_context(|| format!("setting file permissions on {image_path}"))?;
            continue;
        }
        bail!("unsupported rootfs entry type: {}", host_path.display());
    }
    Ok(())
}

fn ext4_path(relative_path: &Path) -> anyhow::Result<String> {
    let path = relative_path
        .to_str()
        .context("rootfs path is not valid UTF-8")?
        .replace('\\', "/");
    Ok(format!("/{}", path))
}

fn copy_staged_partition(
    staging_path: &Path,
    image_path: &Path,
    offset_bytes: u64,
) -> anyhow::Result<()> {
    let mut staging =
        File::open(staging_path).with_context(|| format!("opening {}", staging_path.display()))?;
    let mut image = File::options()
        .write(true)
        .open(image_path)
        .with_context(|| format!("opening {} for partition copy", image_path.display()))?;
    image
        .seek(std::io::SeekFrom::Start(offset_bytes))
        .with_context(|| format!("seeking {}", image_path.display()))?;
    io::copy(&mut staging, &mut image)
        .with_context(|| format!("copying staged ext4 image into {}", image_path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::io::Write;
    use std::path::Path;

    use flate2::Compression;
    use flate2::write::GzEncoder;
    use tar::{Builder, Header};
    use tempfile::tempdir;

    use super::unpack_with_tar;

    #[test]
    fn unpacks_multi_member_gzip_tar() -> anyhow::Result<()> {
        let temp_dir = tempdir()?;
        let archive_path = temp_dir.path().join("rootfs.tar.gz");
        let output_dir = temp_dir.path().join("rootfs");

        let tar_bytes = build_tar(&[
            (
                "etc/inittab",
                b"::sysinit:/bin/mount -t proc proc /proc\n" as &[u8],
            ),
            (
                "usr/local/bin/sagens-guest-agent",
                b"#!/bin/sh\necho ok\n" as &[u8],
            ),
        ])?;
        write_multi_member_gzip(&archive_path, &tar_bytes, 1024)?;

        unpack_with_tar(&archive_path, &output_dir)?;

        assert_eq!(
            std::fs::read_to_string(output_dir.join("etc/inittab"))?,
            "::sysinit:/bin/mount -t proc proc /proc\n"
        );
        assert_eq!(
            std::fs::read(output_dir.join("usr/local/bin/sagens-guest-agent"))?,
            b"#!/bin/sh\necho ok\n"
        );
        Ok(())
    }

    fn build_tar(entries: &[(&str, &[u8])]) -> anyhow::Result<Vec<u8>> {
        let mut tar_bytes = Vec::new();
        {
            let mut builder = Builder::new(&mut tar_bytes);
            for (path, data) in entries {
                let mut header = Header::new_gnu();
                header.set_path(path)?;
                header.set_mode(0o755);
                header.set_size(data.len() as u64);
                header.set_cksum();
                builder.append(&header, *data)?;
            }
            builder.finish()?;
        }
        Ok(tar_bytes)
    }

    fn write_multi_member_gzip(path: &Path, bytes: &[u8], split_at: usize) -> anyhow::Result<()> {
        assert!(split_at < bytes.len());
        let first = gzip_member(&bytes[..split_at])?;
        let second = gzip_member(&bytes[split_at..])?;
        let mut archive = File::create(path)?;
        archive.write_all(&first)?;
        archive.write_all(&second)?;
        Ok(())
    }

    fn gzip_member(bytes: &[u8]) -> anyhow::Result<Vec<u8>> {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(bytes)?;
        Ok(encoder.finish()?)
    }
}
