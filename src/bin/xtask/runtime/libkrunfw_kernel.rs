use std::fs;
use std::path::Path;

use anyhow::{Context, ensure};
use object::{Object, ObjectSection, ObjectSymbol};
use tempfile::tempdir;

use crate::cargo_ops::run;

const PREBUILT_VERSION: &str = "v5.2.0";
const PREBUILT_AARCH64_ARCHIVE: &str = "libkrunfw-prebuilt-aarch64.tgz";
const PREBUILT_AARCH64_URL: &str = "https://github.com/containers/libkrunfw/releases/download/v5.2.0/libkrunfw-prebuilt-aarch64.tgz";
const PREBUILT_X86_64_ARCHIVE: &str = "libkrunfw-x86_64.tgz";
const PREBUILT_X86_64_URL: &str =
    "https://github.com/containers/libkrunfw/releases/download/v5.2.0/libkrunfw-x86_64.tgz";
const KERNEL_C_PATH: &str = "libkrunfw/kernel.c";
const X86_64_LIBRARY_PATH: &str = "lib64/libkrunfw.so";
const KERNEL_BUNDLE_SYMBOL: &str = "KERNEL_BUNDLE";
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

pub(super) fn materialize_linux_x86_64_guest_kernel(
    work_dir: &Path,
    output_dir: &Path,
) -> anyhow::Result<()> {
    let archive_path = cached_prebuilt_archive(
        work_dir,
        "libkrunfw-x86_64",
        PREBUILT_X86_64_ARCHIVE,
        PREBUILT_X86_64_URL,
    )?;
    let extract_dir = tempdir().context("creating libkrunfw extraction directory")?;
    extract_archive_member(&archive_path, X86_64_LIBRARY_PATH, extract_dir.path())?;
    let kernel = parse_kernel_bundle_elf(&extract_dir.path().join(X86_64_LIBRARY_PATH))?;

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

fn parse_kernel_bundle_elf(path: &Path) -> anyhow::Result<Vec<u8>> {
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let elf = object::File::parse(bytes.as_slice())
        .with_context(|| format!("parsing ELF image {}", path.display()))?;
    let symbol = elf
        .dynamic_symbols()
        .chain(elf.symbols())
        .find(|symbol| matches!(symbol.name(), Ok(name) if name == KERNEL_BUNDLE_SYMBOL))
        .context("locating KERNEL_BUNDLE symbol")?;
    let section_index = symbol
        .section_index()
        .context("KERNEL_BUNDLE is not backed by an ELF section")?;
    let section = elf
        .section_by_index(section_index)
        .context("reading ELF section for KERNEL_BUNDLE")?;
    let section_data = section.uncompressed_data().with_context(|| {
        format!(
            "reading {} section bytes",
            section.name().unwrap_or("<unnamed>")
        )
    })?;
    let start = usize::try_from(
        symbol
            .address()
            .checked_sub(section.address())
            .context("KERNEL_BUNDLE starts before its section base")?,
    )
    .context("converting KERNEL_BUNDLE offset")?;
    let size = usize::try_from(symbol.size()).context("converting KERNEL_BUNDLE size")?;
    ensure!(size > 0, "KERNEL_BUNDLE symbol is empty");
    let end = start
        .checked_add(size)
        .context("KERNEL_BUNDLE range overflowed section bounds")?;
    ensure!(
        end <= section_data.len(),
        "KERNEL_BUNDLE range {}..{} exceeds {} section size {}",
        start,
        end,
        section.name().unwrap_or("<unnamed>"),
        section_data.len(),
    );
    let mut kernel = section_data[start..end].to_vec();
    if kernel.last() == Some(&0) {
        kernel.pop();
    }
    ensure!(!kernel.is_empty(), "decoded KERNEL_BUNDLE is empty");
    Ok(kernel)
}
