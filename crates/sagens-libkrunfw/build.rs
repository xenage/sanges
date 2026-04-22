use std::env;
use std::ffi::c_char;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

type GetKernelFn = unsafe extern "C" fn(*mut u64, *mut u64, *mut usize) -> *mut c_char;

const PREBUILT_VERSION: &str = "v5.2.0";
const PREBUILT_ARCHIVE: &str = "libkrunfw-x86_64.tgz";
const PREBUILT_URL: &str =
    "https://github.com/containers/libkrunfw/releases/download/v5.2.0/libkrunfw-x86_64.tgz";
const SHARED_OBJECT_PATH: &str = "lib64/libkrunfw.so.5.2.0";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("linux")
        || env::var("CARGO_CFG_TARGET_ARCH").as_deref() != Ok("x86_64")
    {
        return;
    }

    if let Err(error) = build_static_kernel_bundle() {
        panic!("building static libkrunfw bundle failed: {error}");
    }
}

fn build_static_kernel_bundle() -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").ok_or("missing OUT_DIR")?);
    let archive_path = out_dir.join(PREBUILT_ARCHIVE);
    let extract_dir = out_dir.join("libkrunfw-extract");
    let shared_object_path = extract_dir.join(SHARED_OBJECT_PATH);
    let kernel_output = out_dir.join("kernel.bin");
    let generated_output = out_dir.join("generated.rs");

    download_archive(&archive_path)?;
    extract_shared_object(&archive_path, &extract_dir)?;
    let (kernel, load_addr, entry_addr) = read_kernel_bundle(&shared_object_path)?;
    fs::write(&kernel_output, kernel)?;
    fs::write(
        &generated_output,
        render_rust_source(&kernel_output, load_addr, entry_addr),
    )?;

    println!("cargo:rerun-if-changed={}", generated_output.display());
    Ok(())
}

fn download_archive(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
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
            .arg(PREBUILT_URL),
        &format!("download libkrunfw prebuilt archive {PREBUILT_VERSION}"),
    )
}

fn extract_shared_object(
    archive_path: &Path,
    output_dir: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    if output_dir.join(SHARED_OBJECT_PATH).is_file() {
        return Ok(());
    }
    fs::create_dir_all(output_dir)?;
    run_command(
        Command::new("tar")
            .arg("-xzf")
            .arg(archive_path)
            .arg("-C")
            .arg(output_dir)
            .arg(SHARED_OBJECT_PATH),
        &format!(
            "extract {SHARED_OBJECT_PATH} from {}",
            archive_path.display()
        ),
    )
}

fn read_kernel_bundle(
    shared_object_path: &Path,
) -> Result<(Vec<u8>, u64, u64), Box<dyn std::error::Error>> {
    let library = unsafe { libloading::Library::new(shared_object_path) }?;
    let get_kernel = unsafe { library.get::<GetKernelFn>(b"krunfw_get_kernel") }?;
    let mut load_addr = 0u64;
    let mut entry_addr = 0u64;
    let mut size = 0usize;
    let host_addr = unsafe {
        get_kernel(
            &mut load_addr as *mut u64,
            &mut entry_addr as *mut u64,
            &mut size as *mut usize,
        )
    };
    if host_addr.is_null() || size == 0 {
        return Err("krunfw_get_kernel returned an empty bundle".into());
    }
    let bytes = unsafe { std::slice::from_raw_parts(host_addr.cast::<u8>(), size) }.to_vec();
    Ok((bytes, load_addr, entry_addr))
}

fn render_rust_source(kernel_output: &Path, load_addr: u64, entry_addr: u64) -> String {
    let kernel_path = kernel_output.display();
    format!(
        r#"use core::ffi::c_char;

#[repr(C, align(4096))]
struct AlignedKernelBundle<const N: usize>([u8; N]);

static KERNEL_BUNDLE: AlignedKernelBundle<{{KERNEL_SIZE}}> =
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
            *size = KERNEL_BUNDLE.0.len();
        }}
    }}
    KERNEL_BUNDLE.0.as_ptr().cast_mut().cast()
}}
"#,
    )
    .replace(
        "{KERNEL_SIZE}",
        &fs::metadata(kernel_output)
            .map(|m| m.len().to_string())
            .unwrap_or_else(|_| "0".to_string()),
    )
}

fn run_command(command: &mut Command, description: &str) -> Result<(), Box<dyn std::error::Error>> {
    let status = command.status()?;
    if !status.success() {
        return Err(format!("{description} failed with status {status}").into());
    }
    Ok(())
}
