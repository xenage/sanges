use std::io::Cursor;
use std::path::{Path, PathBuf};
#[cfg(target_os = "macos")]
use std::slice;

#[cfg(target_os = "macos")]
use libloading::{Library, Symbol};
use tokio::fs;
use tokio::io::AsyncReadExt;
use tokio::task;

use crate::{GuestConfig, GuestKernelFormat, Result, SandboxError};

#[derive(Clone, Copy)]
pub enum Compression {
    None,
    Zstd,
}

#[derive(Clone, Copy)]
pub struct EmbeddedAsset {
    pub file_name: &'static str,
    pub bytes: &'static [u8],
    pub compression: Compression,
    pub mode: u32,
}

include!(concat!(env!("OUT_DIR"), "/embedded_bundle.rs"));

const KERNEL_FORMAT_PROBE_BYTES: usize = 256 * 1024;

pub fn has_embedded_assets() -> bool {
    LIBKRUN.is_some()
        || KERNEL.is_some()
        || ROOTFS.is_some()
        || FIRMWARE.is_some()
        || !RUNTIME_SUPPORT.is_empty()
}

pub async fn resolve_guest_paths(
    state_dir: &Path,
    bundle_id: &str,
    guest: &GuestConfig,
) -> Result<GuestConfig> {
    let bundle_dir = state_dir.join("embedded-bundle").join(bundle_id);
    extract_runtime_support_assets(&bundle_dir, RUNTIME_SUPPORT).await?;
    let libkrun_library = resolve_path(&bundle_dir, &guest.libkrun_library, LIBKRUN).await?;
    let kernel_image = resolve_optional_path(&bundle_dir, &guest.kernel_image, KERNEL).await?;
    let rootfs_image = resolve_path(&bundle_dir, &guest.rootfs_image, ROOTFS).await?;
    let firmware = match &guest.firmware {
        Some(path) if !path.as_os_str().is_empty() => Some(path.clone()),
        _ => match FIRMWARE {
            Some(asset) => Some(extract_asset(&bundle_dir, "firmware", asset).await?),
            None => None,
        },
    };
    let (kernel_image, kernel_format) = resolve_kernel_path(
        &bundle_dir,
        &libkrun_library,
        kernel_image,
        guest.kernel_format,
    )
    .await?;
    let kernel_format = detect_kernel_format(&kernel_image, kernel_format).await?;
    Ok(GuestConfig {
        libkrun_library,
        kernel_image,
        kernel_format,
        rootfs_image,
        firmware,
        guest_agent_path: guest.guest_agent_path.clone(),
        guest_vsock_port: guest.guest_vsock_port,
        boot_timeout: guest.boot_timeout,
        guest_uid: guest.guest_uid,
        guest_gid: guest.guest_gid,
        guest_tmpfs_mib: guest.guest_tmpfs_mib,
    })
}

async fn resolve_path(
    bundle_dir: &Path,
    configured: &Path,
    embedded: Option<EmbeddedAsset>,
) -> Result<PathBuf> {
    if !configured.as_os_str().is_empty() {
        return Ok(configured.to_path_buf());
    }
    match embedded {
        Some(asset) => extract_asset(bundle_dir, asset.file_name, asset).await,
        None => Err(SandboxError::invalid(format!(
            "missing required embedded asset and no explicit path was configured for {}",
            bundle_dir.display()
        ))),
    }
}

async fn resolve_optional_path(
    bundle_dir: &Path,
    configured: &Path,
    embedded: Option<EmbeddedAsset>,
) -> Result<PathBuf> {
    if !configured.as_os_str().is_empty() {
        return Ok(configured.to_path_buf());
    }
    match embedded {
        Some(asset) => extract_asset(bundle_dir, asset.file_name, asset).await,
        None => Ok(PathBuf::new()),
    }
}

async fn extract_asset(bundle_dir: &Path, label: &str, asset: EmbeddedAsset) -> Result<PathBuf> {
    fs::create_dir_all(bundle_dir)
        .await
        .map_err(|error| SandboxError::io("creating embedded bundle directory", error))?;
    let path = bundle_dir.join(asset.file_name);
    let bytes = materialize_asset(asset).await?;
    fs::write(&path, bytes)
        .await
        .map_err(|error| SandboxError::io(format!("writing embedded asset {label}"), error))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        fs::set_permissions(&path, std::fs::Permissions::from_mode(asset.mode))
            .await
            .map_err(|error| {
                SandboxError::io(format!("setting embedded asset mode {label}"), error)
            })?;
    }
    Ok(path)
}

async fn materialize_asset(asset: EmbeddedAsset) -> Result<Vec<u8>> {
    match asset.compression {
        Compression::None => Ok(asset.bytes.to_vec()),
        Compression::Zstd => {
            let bytes = asset.bytes;
            task::spawn_blocking(move || {
                zstd::stream::decode_all(Cursor::new(bytes)).map_err(|error| {
                    SandboxError::backend(format!(
                        "decompressing embedded asset {}: {error}",
                        asset.file_name
                    ))
                })
            })
            .await
            .map_err(|error| {
                SandboxError::backend(format!(
                    "joining embedded asset decompression task for {}: {error}",
                    asset.file_name
                ))
            })?
        }
    }
}

async fn extract_runtime_support_assets(bundle_dir: &Path, assets: &[EmbeddedAsset]) -> Result<()> {
    for asset in assets {
        extract_asset(bundle_dir, asset.file_name, *asset).await?;
    }
    Ok(())
}

async fn detect_kernel_format(
    path: &Path,
    fallback: GuestKernelFormat,
) -> Result<GuestKernelFormat> {
    if path.as_os_str().is_empty() {
        return Ok(fallback);
    }
    let mut file = match fs::File::open(path).await {
        Ok(file) => file,
        Err(_) => return Ok(fallback),
    };
    let mut probe = vec![0u8; KERNEL_FORMAT_PROBE_BYTES];
    let read = match file.read(&mut probe).await {
        Ok(read) => read,
        Err(_) => return Ok(fallback),
    };
    probe.truncate(read);
    Ok(detect_kernel_format_from_probe(&probe, fallback))
}

fn detect_kernel_format_from_probe(probe: &[u8], fallback: GuestKernelFormat) -> GuestKernelFormat {
    if probe.starts_with(b"\x7fELF") {
        return GuestKernelFormat::Elf;
    }
    // Linux x86_64 vmlinuz images are EFI/PE-wrapped and usually embed the
    // compressed ELF payload behind the DOS/PE stub. Treat them as Image*
    // kernels so libkrun loads the embedded ELF instead of executing the PE
    // header as a raw kernel blob.
    if probe.starts_with(b"MZ") {
        if probe.windows(3).any(|window| window == [0x1f, 0x8b, 0x08]) {
            return GuestKernelFormat::ImageGz;
        }
        if probe.windows(3).any(|window| window == *b"BZh") {
            return GuestKernelFormat::ImageBz2;
        }
        if probe
            .windows(4)
            .any(|window| window == [0x28, 0xb5, 0x2f, 0xfd])
        {
            return GuestKernelFormat::ImageZstd;
        }
    }
    fallback
}

#[cfg(target_os = "macos")]
async fn resolve_kernel_path(
    bundle_dir: &Path,
    libkrun_library: &Path,
    kernel_image: PathBuf,
    kernel_format: GuestKernelFormat,
) -> Result<(PathBuf, GuestKernelFormat)> {
    if matches!(
        std::env::var("SAGENS_USE_LIBKRUNFW_KERNEL").ok().as_deref(),
        Some("0" | "false" | "no" | "off")
    ) {
        return Ok((kernel_image, kernel_format));
    }
    let libkrunfw = match find_libkrunfw(libkrun_library) {
        Some(path) => path,
        None => return Ok((kernel_image, kernel_format)),
    };
    let extracted = extract_libkrunfw_kernel(bundle_dir, &libkrunfw).await?;
    Ok((extracted, GuestKernelFormat::Raw))
}

#[cfg(not(target_os = "macos"))]
async fn resolve_kernel_path(
    _: &Path,
    _: &Path,
    kernel_image: PathBuf,
    kernel_format: GuestKernelFormat,
) -> Result<(PathBuf, GuestKernelFormat)> {
    Ok((kernel_image, kernel_format))
}

#[cfg(target_os = "macos")]
fn find_libkrunfw(libkrun_library: &Path) -> Option<PathBuf> {
    let directory = libkrun_library.parent()?;
    for name in ["libkrunfw.5.dylib", "libkrunfw.dylib"] {
        let candidate = directory.join(name);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

#[cfg(target_os = "macos")]
async fn extract_libkrunfw_kernel(bundle_dir: &Path, libkrunfw: &Path) -> Result<PathBuf> {
    fs::create_dir_all(bundle_dir)
        .await
        .map_err(|error| SandboxError::io("creating embedded bundle directory", error))?;
    let path = bundle_dir.join("libkrunfw-kernel.Image");
    if path.exists() {
        return Ok(path);
    }
    let bytes = read_libkrunfw_kernel(libkrunfw)?;
    fs::write(&path, bytes)
        .await
        .map_err(|error| SandboxError::io("writing libkrunfw kernel image", error))?;
    Ok(path)
}

#[cfg(target_os = "macos")]
fn read_libkrunfw_kernel(libkrunfw: &Path) -> Result<Vec<u8>> {
    type KrunfwGetKernel =
        unsafe extern "C" fn(*mut usize, *mut usize, *mut usize) -> *mut std::ffi::c_char;

    let library = unsafe { Library::new(libkrunfw) }
        .map_err(|error| SandboxError::backend(format!("loading libkrunfw: {error}")))?;
    let symbol: Symbol<'_, KrunfwGetKernel> = unsafe { library.get(b"krunfw_get_kernel\0") }
        .map_err(|error| SandboxError::backend(format!("loading krunfw_get_kernel: {error}")))?;
    let mut load_addr = 0usize;
    let mut entry_addr = 0usize;
    let mut size = 0usize;
    let pointer = unsafe { symbol(&mut load_addr, &mut entry_addr, &mut size) };
    if pointer.is_null() || size == 0 {
        return Err(SandboxError::backend(
            "libkrunfw returned an empty kernel bundle",
        ));
    }
    if load_addr == 0 || entry_addr == 0 {
        return Err(SandboxError::backend(
            "libkrunfw returned an invalid kernel load address",
        ));
    }
    Ok(unsafe { slice::from_raw_parts(pointer.cast::<u8>(), size) }.to_vec())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::Duration;

    use tempfile::tempdir;

    use super::{detect_kernel_format_from_probe, resolve_guest_paths};
    use crate::{GuestConfig, GuestKernelFormat};

    #[tokio::test]
    async fn keeps_explicit_paths_when_present() {
        let temp = tempdir().expect("tempdir");
        let guest = GuestConfig {
            libkrun_library: PathBuf::from("/tmp/libkrun.dylib"),
            kernel_image: PathBuf::from("/tmp/kernel"),
            kernel_format: crate::GuestKernelFormat::Raw,
            rootfs_image: PathBuf::from("/tmp/rootfs.raw"),
            firmware: Some(PathBuf::from("/tmp/firmware.fd")),
            guest_agent_path: PathBuf::from("/usr/local/bin/sagens-guest-agent"),
            guest_vsock_port: 11_000,
            boot_timeout: Duration::from_secs(30),
            guest_uid: 65_534,
            guest_gid: 65_534,
            guest_tmpfs_mib: 256,
        };
        let resolved = resolve_guest_paths(temp.path(), "test", &guest)
            .await
            .expect("resolve");
        assert_eq!(resolved.libkrun_library, guest.libkrun_library);
        assert_eq!(resolved.kernel_image, guest.kernel_image);
        assert_eq!(resolved.rootfs_image, guest.rootfs_image);
        assert_eq!(resolved.firmware, guest.firmware);
    }

    #[tokio::test]
    async fn rejects_missing_required_assets_without_bundle() {
        let temp = tempdir().expect("tempdir");
        let result = resolve_guest_paths(
            temp.path(),
            "missing",
            &GuestConfig {
                libkrun_library: PathBuf::new(),
                kernel_image: PathBuf::new(),
                kernel_format: crate::GuestKernelFormat::Raw,
                rootfs_image: PathBuf::new(),
                firmware: None,
                guest_agent_path: PathBuf::from("/usr/local/bin/sagens-guest-agent"),
                guest_vsock_port: 11_000,
                boot_timeout: Duration::from_secs(30),
                guest_uid: 65_534,
                guest_gid: 65_534,
                guest_tmpfs_mib: 256,
            },
        )
        .await;
        if super::has_embedded_assets() {
            assert!(result.is_ok());
        } else {
            assert!(result.is_err());
        }
    }

    #[test]
    fn detects_embedded_elf_payload_in_pe_wrapped_linux_kernel() {
        let probe = b"MZ\x00\x00stub\x1f\x8b\x08rest";
        assert_eq!(
            detect_kernel_format_from_probe(probe, GuestKernelFormat::Raw),
            GuestKernelFormat::ImageGz
        );
    }

    #[test]
    fn keeps_explicit_pe_gz_fallback_for_plain_gzip_kernel_files() {
        let probe = b"\x1f\x8b\x08plain gzip stream";
        assert_eq!(
            detect_kernel_format_from_probe(probe, GuestKernelFormat::PeGz),
            GuestKernelFormat::PeGz
        );
    }

    #[tokio::test]
    async fn detects_linux_x86_kernel_format_from_explicit_vmlinuz_path() {
        let temp = tempdir().expect("tempdir");
        let kernel = temp.path().join("vmlinuz-virt");
        fs::write(&kernel, b"MZ\x00\x00stub\x1f\x8b\x08payload").expect("write kernel");
        let guest = GuestConfig {
            libkrun_library: PathBuf::from("/tmp/libkrun.so"),
            kernel_image: kernel.clone(),
            kernel_format: GuestKernelFormat::Raw,
            rootfs_image: PathBuf::from("/tmp/rootfs.raw"),
            firmware: None,
            guest_agent_path: PathBuf::from("/usr/local/bin/sagens-guest-agent"),
            guest_vsock_port: 11_000,
            boot_timeout: Duration::from_secs(30),
            guest_uid: 65_534,
            guest_gid: 65_534,
            guest_tmpfs_mib: 256,
        };
        let resolved = resolve_guest_paths(temp.path(), "test", &guest)
            .await
            .expect("resolve");
        assert_eq!(resolved.kernel_image, kernel);
        assert_eq!(resolved.kernel_format, GuestKernelFormat::ImageGz);
    }
}
