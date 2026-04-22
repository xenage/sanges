use std::path::{Path, PathBuf};

use crate::backend::libkrun::config::LibkrunRunnerConfig;
use crate::{Result, SandboxError};

pub(super) fn kernel_image_for_libkrun(config: &LibkrunRunnerConfig) -> Result<PathBuf> {
    if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        return pad_kernel_file_for_mmap(
            &config.kernel_image,
            &config.runtime_dir,
            host_page_size()?,
        );
    }
    Ok(config.kernel_image.clone())
}

pub(super) fn pad_kernel_file_for_mmap(
    kernel_image: &Path,
    runtime_dir: &Path,
    page_size: usize,
) -> Result<PathBuf> {
    let metadata = std::fs::metadata(kernel_image)
        .map_err(|error| SandboxError::io("reading kernel image metadata", error))?;
    let kernel_size = usize::try_from(metadata.len())
        .map_err(|_| SandboxError::invalid("kernel image is too large to map"))?;
    let aligned_size = kernel_size.div_ceil(page_size) * page_size;
    if aligned_size == kernel_size {
        return Ok(kernel_image.to_path_buf());
    }
    std::fs::create_dir_all(runtime_dir)
        .map_err(|error| SandboxError::io("creating libkrun runtime directory", error))?;
    let file_name = kernel_image
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("kernel.raw");
    let padded_path = runtime_dir.join(format!("{file_name}.page-aligned"));
    std::fs::copy(kernel_image, &padded_path)
        .map_err(|error| SandboxError::io("copying kernel image for libkrun mmap", error))?;
    let padded = std::fs::OpenOptions::new()
        .write(true)
        .open(&padded_path)
        .map_err(|error| SandboxError::io("opening padded kernel image", error))?;
    padded
        .set_len(aligned_size as u64)
        .map_err(|error| SandboxError::io("extending padded kernel image", error))?;
    Ok(padded_path)
}

fn host_page_size() -> Result<usize> {
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if page_size <= 0 {
        return Err(SandboxError::backend(
            "failed to determine host page size for libkrun kernel mmap",
        ));
    }
    Ok(page_size as usize)
}
