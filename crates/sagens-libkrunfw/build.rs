use object::{Object, ObjectSection, ObjectSymbol};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const PREBUILT_VERSION: &str = "v5.2.0";
const PREBUILT_AARCH64_ARCHIVE: &str = "libkrunfw-prebuilt-aarch64.tgz";
const PREBUILT_AARCH64_URL: &str = "https://github.com/containers/libkrunfw/releases/download/v5.2.0/libkrunfw-prebuilt-aarch64.tgz";
const PREBUILT_X86_64_ARCHIVE: &str = "libkrunfw-x86_64.tgz";
const PREBUILT_X86_64_URL: &str =
    "https://github.com/containers/libkrunfw/releases/download/v5.2.0/libkrunfw-x86_64.tgz";
const KERNEL_C_PATH: &str = "libkrunfw/kernel.c";
const X86_64_LIBRARY_PATH: &str = "lib64/libkrunfw.so.5.2.0";
const KERNEL_BUNDLE_SYMBOL: &str = "KERNEL_BUNDLE";
const KRUNFW_GET_KERNEL_SYMBOL: &str = "krunfw_get_kernel";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    if target_os != "linux" || !matches!(target_arch.as_str(), "x86_64" | "aarch64") {
        return;
    }

    if let Err(error) = build_static_kernel_bundle(&target_arch) {
        panic!("building static libkrunfw bundle failed: {error}");
    }
}

fn build_static_kernel_bundle(target_arch: &str) -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").ok_or("missing OUT_DIR")?);
    let kernel_output = out_dir.join("kernel.bin");
    let generated_output = out_dir.join("generated.rs");
    let (archive_name, archive_url, archive_member) = match target_arch {
        "x86_64" => (
            PREBUILT_X86_64_ARCHIVE,
            PREBUILT_X86_64_URL,
            X86_64_LIBRARY_PATH,
        ),
        "aarch64" => (
            PREBUILT_AARCH64_ARCHIVE,
            PREBUILT_AARCH64_URL,
            KERNEL_C_PATH,
        ),
        other => return Err(format!("unsupported libkrunfw target arch {other}").into()),
    };
    let archive_path = out_dir.join(archive_name);
    let extract_dir = out_dir.join("libkrunfw-extract");
    let extracted_path = extract_dir.join(archive_member);

    download_archive(&archive_path, archive_url)?;
    extract_archive_member(&archive_path, archive_member, &extract_dir)?;
    let (kernel, load_addr, entry_addr) = match target_arch {
        "x86_64" => parse_kernel_bundle_elf(&extracted_path)?,
        "aarch64" => parse_kernel_bundle_c(&extracted_path)?,
        _ => unreachable!(),
    };

    fs::write(&kernel_output, kernel)?;
    fs::write(
        &generated_output,
        render_rust_source(&kernel_output, load_addr, entry_addr),
    )?;
    Ok(())
}

fn download_archive(path: &Path, url: &str) -> Result<(), Box<dyn std::error::Error>> {
    if path.is_file() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    run_command(
        Command::new("curl")
            .arg("-L")
            .arg("--fail")
            .arg("-o")
            .arg(path)
            .arg(url),
        &format!("download libkrunfw prebuilt archive {PREBUILT_VERSION}"),
    )
}

fn extract_archive_member(
    archive_path: &Path,
    member: &str,
    output_dir: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    if output_dir.join(member).is_file() {
        return Ok(());
    }
    fs::create_dir_all(output_dir)?;
    run_command(
        Command::new("tar")
            .arg("-xzf")
            .arg(archive_path)
            .arg("-C")
            .arg(output_dir)
            .arg(member),
        &format!("extract {member} from {}", archive_path.display()),
    )
}

fn parse_kernel_bundle_c(path: &Path) -> Result<(Vec<u8>, u64, u64), Box<dyn std::error::Error>> {
    let source = fs::read_to_string(path)?;
    let start = source
        .find("char KERNEL_BUNDLE[] = \n\"")
        .ok_or("locating KERNEL_BUNDLE declaration")?
        + "char KERNEL_BUNDLE[] = \n\"".len();
    let end = source[start..]
        .find("\";\n\nchar * krunfw_get_kernel")
        .ok_or("locating KERNEL_BUNDLE terminator")?
        + start;
    let escaped = source[start..end].replace("\"\n\"", "");
    let kernel = decode_c_hex_bundle(&escaped)?;
    let load_addr = parse_c_assignment(&source, "*load_addr = ")?;
    let entry_addr = parse_c_assignment(&source, "*entry_addr = ")?;
    Ok((kernel, load_addr, entry_addr))
}

fn decode_c_hex_bundle(escaped: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let bytes = escaped.as_bytes();
    let mut output = Vec::with_capacity(bytes.len() / 4);
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes.get(index..index + 2) != Some(br"\x") {
            return Err(format!("unexpected kernel bundle token at byte {index}").into());
        }
        index += 2;
        let start = index;
        while index < bytes.len() && bytes[index].is_ascii_hexdigit() {
            index += 1;
        }
        if index == start {
            return Err(format!("missing hex byte at byte {start}").into());
        }
        let value = std::str::from_utf8(&bytes[start..index])?;
        output.push(u8::from_str_radix(value, 16)?);
    }
    Ok(output)
}

fn parse_c_assignment(source: &str, prefix: &str) -> Result<u64, Box<dyn std::error::Error>> {
    let start = source.find(prefix).ok_or(prefix)? + prefix.len();
    let end = source[start..].find(';').ok_or("assignment terminator")? + start;
    let value = source[start..end].trim();
    if let Some(value) = value.strip_prefix("0x") {
        return Ok(u64::from_str_radix(value, 16)?);
    }
    Ok(value.parse()?)
}

fn parse_kernel_bundle_elf(path: &Path) -> Result<(Vec<u8>, u64, u64), Box<dyn std::error::Error>> {
    let bytes = fs::read(path)?;
    let elf = object::File::parse(bytes.as_slice())?;
    let symbol = elf
        .dynamic_symbols()
        .chain(elf.symbols())
        .find(|symbol| matches!(symbol.name(), Ok(name) if name == KERNEL_BUNDLE_SYMBOL))
        .ok_or("locating KERNEL_BUNDLE symbol")?;
    let section_index = symbol
        .section_index()
        .ok_or("KERNEL_BUNDLE is not backed by an ELF section")?;
    let section = elf.section_by_index(section_index)?;
    let section_data = section.uncompressed_data()?;
    let start = usize::try_from(
        symbol
            .address()
            .checked_sub(section.address())
            .ok_or("KERNEL_BUNDLE starts before its section base")?,
    )?;
    let size = usize::try_from(symbol.size())?;
    let end = start
        .checked_add(size)
        .ok_or("KERNEL_BUNDLE range overflow")?;
    if end > section_data.len() {
        return Err(format!("KERNEL_BUNDLE range {start}..{end} exceeds section size").into());
    }
    let mut kernel = section_data[start..end].to_vec();
    if kernel.last() == Some(&0) {
        kernel.pop();
    }
    if kernel.is_empty() {
        return Err("decoded KERNEL_BUNDLE is empty".into());
    }

    let function = read_symbol_bytes(&elf, KRUNFW_GET_KERNEL_SYMBOL)?;
    let immediates = function
        .windows(7)
        .filter_map(|window| {
            if window[0..3] != [0x48, 0xc7, 0x00] {
                return None;
            }
            Some(u32::from_le_bytes(window[3..7].try_into().ok()?) as u64)
        })
        .collect::<Vec<_>>();
    if immediates.len() < 3 {
        return Err("failed to decode krunfw_get_kernel constants".into());
    }
    let load_addr = immediates[0];
    let entry_addr = immediates[1];
    let expected_size = usize::try_from(immediates[2])?;
    if kernel.len() != expected_size {
        return Err(format!(
            "decoded KERNEL_BUNDLE size {} does not match krunfw_get_kernel size {expected_size}",
            kernel.len()
        )
        .into());
    }
    Ok((kernel, load_addr, entry_addr))
}

fn read_symbol_bytes(
    elf: &object::File<'_>,
    symbol_name: &str,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let symbol = elf
        .dynamic_symbols()
        .chain(elf.symbols())
        .find(|symbol| matches!(symbol.name(), Ok(name) if name == symbol_name))
        .ok_or_else(|| format!("locating {symbol_name} symbol"))?;
    let section_index = symbol
        .section_index()
        .ok_or_else(|| format!("{symbol_name} is not backed by an ELF section"))?;
    let section = elf.section_by_index(section_index)?;
    let section_data = section.uncompressed_data()?;
    let start = usize::try_from(
        symbol
            .address()
            .checked_sub(section.address())
            .ok_or_else(|| format!("{symbol_name} starts before its section base"))?,
    )?;
    let size = usize::try_from(symbol.size())?;
    let end = start
        .checked_add(size)
        .ok_or_else(|| format!("{symbol_name} range overflow"))?;
    if end > section_data.len() {
        return Err(format!("{symbol_name} range exceeds section size").into());
    }
    Ok(section_data[start..end].to_vec())
}

fn render_rust_source(kernel_output: &Path, load_addr: u64, entry_addr: u64) -> String {
    let kernel_size = fs::metadata(kernel_output)
        .map(|metadata| metadata.len().to_string())
        .unwrap_or_else(|_| "0".to_string());
    format!(
        r#"use core::ffi::c_char;

#[repr(C, align(65536))]
struct AlignedKernelBundle<const N: usize>([u8; N]);

static mut KERNEL_BUNDLE: AlignedKernelBundle<{kernel_size}> =
    AlignedKernelBundle(*include_bytes!("{kernel_path}"));

pub static STATIC_KRUNFW_LINKED: () = ();

#[unsafe(no_mangle)]
pub unsafe extern "C" fn krunfw_get_kernel(
    load_addr: *mut u64,
    entry_addr: *mut u64,
    size: *mut usize,
) -> *mut c_char {{
    if !load_addr.is_null() {{
        unsafe {{
            *load_addr = {load_addr:#x};
        }}
    }}
    if !entry_addr.is_null() {{
        unsafe {{
            *entry_addr = {entry_addr:#x};
        }}
    }}
    if !size.is_null() {{
        unsafe {{
            *size = {kernel_size};
        }}
    }}
    core::ptr::addr_of_mut!(KERNEL_BUNDLE.0).cast::<u8>().cast::<c_char>()
}}
"#,
        kernel_path = kernel_output.display(),
    )
}

fn run_command(command: &mut Command, description: &str) -> Result<(), Box<dyn std::error::Error>> {
    let status = command.status()?;
    if !status.success() {
        return Err(format!("{description} failed with status {status}").into());
    }
    Ok(())
}
