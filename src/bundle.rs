use std::io::Cursor;
use std::path::{Path, PathBuf};
#[cfg(target_os = "macos")]
use std::slice;

#[cfg(target_os = "macos")]
use libloading::{Library, Symbol};
use tokio::fs;
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
}

include!(concat!(env!("OUT_DIR"), "/embedded_bundle.rs"));

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
    use std::path::PathBuf;
    use std::time::Duration;

    use tempfile::tempdir;

    use super::resolve_guest_paths;
    use crate::GuestConfig;

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
}
