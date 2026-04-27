use std::fs;
use std::path::Path;

use anyhow::{Context, ensure};
use tempfile::tempdir;

use crate::cargo_ops::run;

const PREBUILT_VERSION: &str = "v5.2.0";
const PREBUILT_AARCH64_ARCHIVE: &str = "libkrunfw-prebuilt-aarch64.tgz";
const PREBUILT_AARCH64_URL: &str = "https://github.com/containers/libkrunfw/releases/download/v5.2.0/libkrunfw-prebuilt-aarch64.tgz";
const KERNEL_C_PATH: &str = "libkrunfw/kernel.c";
const KERNEL_OUTPUT: &str = "vmlinuz-virt";

pub(super) fn materialize_macos_aarch64_guest_kernel(
    work_dir: &Path,
    output_dir: &Path,
) -> anyhow::Result<()> {
    let archive_path = cached_prebuilt_archive(
        work_dir,
        "libkrunfw-prebuilt-aarch64",
        PREBUILT_AARCH64_ARCHIVE,
        PREBUILT_AARCH64_URL,
    )?;

    let extract_dir = tempdir().context("creating libkrunfw extraction directory")?;
    extract_kernel_c(&archive_path, extract_dir.path())?;
    let kernel_c = extract_dir.path().join(KERNEL_C_PATH);
    let kernel = parse_kernel_bundle(&kernel_c)?;

    fs::create_dir_all(output_dir).with_context(|| format!("creating {}", output_dir.display()))?;
    fs::write(output_dir.join(KERNEL_OUTPUT), kernel)
        .with_context(|| format!("writing {}", output_dir.join(KERNEL_OUTPUT).display()))?;
    Ok(())
}

fn cached_prebuilt_archive(
    work_dir: &Path,
    cache_key: &str,
    archive_name: &str,
    archive_url: &str,
) -> anyhow::Result<std::path::PathBuf> {
    let cache_dir = work_dir.join(cache_key);
    fs::create_dir_all(&cache_dir).with_context(|| format!("creating {}", cache_dir.display()))?;
    let archive_path = cache_dir.join(archive_name);
    if !archive_path.is_file() {
        download_prebuilt_archive(&archive_path, archive_name, archive_url)?;
    }
    Ok(archive_path)
}

fn download_prebuilt_archive(
    path: &Path,
    archive_name: &str,
    archive_url: &str,
) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .context("archive path has no parent directory")?;
    fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    let mut command = crate::cmd::tool_command("curl");
    command
        .arg("-L")
        .arg("--fail")
        .arg("-o")
        .arg(path)
        .arg(archive_url);
    run(
        command,
        &format!(
            "downloading libkrunfw prebuilt kernel bundle {archive_name} ({PREBUILT_VERSION})"
        ),
    )
}

fn extract_kernel_c(archive_path: &Path, output_dir: &Path) -> anyhow::Result<()> {
    extract_archive_member(archive_path, KERNEL_C_PATH, output_dir)
}

fn extract_archive_member(
    archive_path: &Path,
    member: &str,
    output_dir: &Path,
) -> anyhow::Result<()> {
    let mut command = crate::cmd::tool_command("tar");
    command
        .arg("-xzf")
        .arg(archive_path)
        .arg("-C")
        .arg(output_dir)
        .arg(member);
    run(
        command,
        &format!("extracting {member} from {}", archive_path.display()),
    )
}

fn parse_kernel_bundle(path: &Path) -> anyhow::Result<Vec<u8>> {
    let source = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let start = source
        .find("char KERNEL_BUNDLE[] = \n\"")
        .context("locating KERNEL_BUNDLE declaration")?
        + "char KERNEL_BUNDLE[] = \n\"".len();
    let end = source[start..]
        .find("\";\n\nchar * krunfw_get_kernel")
        .context("locating KERNEL_BUNDLE terminator")?
        + start;
    let escaped = source[start..end].replace("\"\n\"", "");
    decode_c_hex_bundle(&escaped)
}

fn decode_c_hex_bundle(escaped: &str) -> anyhow::Result<Vec<u8>> {
    let bytes = escaped.as_bytes();
    let mut output = Vec::with_capacity(bytes.len() / 4);
    let mut index = 0usize;
    while index < bytes.len() {
        ensure!(
            bytes.get(index..index + 2) == Some(br"\x"),
            "unexpected kernel bundle token at byte {index}"
        );
        index += 2;
        let start = index;
        while index < bytes.len() && bytes[index].is_ascii_hexdigit() {
            index += 1;
        }
        ensure!(index > start, "missing hex byte at byte {start}");
        let value = std::str::from_utf8(&bytes[start..index])
            .context("decoding kernel bundle hex token")?;
        output.push(
            u8::from_str_radix(value, 16)
                .with_context(|| format!("parsing kernel bundle byte {value}"))?,
        );
    }
    Ok(output)
}
