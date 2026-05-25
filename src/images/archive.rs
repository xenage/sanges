use std::fs::{self, File};
use std::io;
use std::path::Path;

use anyhow::{Context, bail};
use flate2::read::MultiGzDecoder;
use tar::Archive;

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

pub(super) fn extract_member_from_tar_gz(
    archive_path: &Path,
    member_name: &str,
    destination: &Path,
) -> anyhow::Result<()> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    let archive =
        File::open(archive_path).with_context(|| format!("opening {}", archive_path.display()))?;
    let decoder = MultiGzDecoder::new(archive);
    let mut archive = Archive::new(decoder);
    for entry in archive
        .entries()
        .with_context(|| format!("reading tar entries from {}", archive_path.display()))?
    {
        let mut entry =
            entry.with_context(|| format!("reading tar entry from {}", archive_path.display()))?;
        let path = entry
            .path()
            .with_context(|| format!("reading tar path from {}", archive_path.display()))?;
        if !archive_member_matches(path.as_ref(), Path::new(member_name)) {
            continue;
        }
        let mut output = File::create(destination)
            .with_context(|| format!("creating {}", destination.display()))?;
        io::copy(&mut entry, &mut output)
            .with_context(|| format!("extracting {member_name} from {}", archive_path.display()))?;
        return Ok(());
    }
    bail!(
        "missing tar member {member_name} in {}",
        archive_path.display()
    )
}

fn archive_member_matches(actual: &Path, expected: &Path) -> bool {
    if actual == expected {
        return true;
    }
    actual
        .strip_prefix(".")
        .map(|path| path == expected)
        .unwrap_or(false)
}
