use std::fs;
use std::path::{Path, PathBuf};

use crate::config::{GuestConfig, GuestKernelFormat};
use crate::{Result, SandboxError};

use super::build::build_image;
use super::types::{
    BASE_APK_PACKAGES, BASE_IMAGE_NAME, IMAGE_MANIFEST, ImageBuildSpec, ResolvedImageGuest,
    VmImageManifest, default_guest_arch, validate_image_name,
};

pub struct ImageStore {
    state_dir: PathBuf,
}

impl ImageStore {
    pub fn new(state_dir: impl Into<PathBuf>) -> Self {
        Self {
            state_dir: state_dir.into(),
        }
    }

    pub fn ensure_layout(&self) -> Result<()> {
        create_dir_all(&self.images_dir(), "creating images directory")?;
        create_dir_all(
            &self.package_cache_dir(),
            "creating package cache directory",
        )
    }

    pub fn build(&self, spec: ImageBuildSpec) -> Result<VmImageManifest> {
        build_image(spec)
    }

    pub fn list(&self) -> Result<Vec<VmImageManifest>> {
        self.ensure_layout()?;
        let mut images = Vec::new();
        let entries = fs::read_dir(self.images_dir())
            .map_err(|error| SandboxError::io("reading image directory", error))?;
        for entry in entries {
            let entry = entry.map_err(|error| SandboxError::io("reading image entry", error))?;
            let path = entry.path().join(IMAGE_MANIFEST);
            if path.is_file() {
                images.push(read_manifest(&path)?);
            }
        }
        images.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(images)
    }

    pub fn inspect(&self, name: &str) -> Result<VmImageManifest> {
        validate_image_name(name)?;
        if name == BASE_IMAGE_NAME {
            return Ok(base_manifest());
        }
        read_manifest(&self.image_dir(name).join(IMAGE_MANIFEST))
    }

    pub fn remove(&self, name: &str) -> Result<()> {
        validate_image_name(name)?;
        if name == BASE_IMAGE_NAME {
            return Err(SandboxError::invalid("base image cannot be removed"));
        }
        let dir = self.image_dir(name);
        if !dir.exists() {
            return Err(SandboxError::not_found(format!("image {name}")));
        }
        fs::remove_dir_all(&dir)
            .map_err(|error| SandboxError::io(format!("removing image {name}"), error))
    }

    pub fn resolve_guest(
        &self,
        name: &str,
        base_guest: &GuestConfig,
    ) -> Result<ResolvedImageGuest> {
        validate_image_name(name)?;
        if name == BASE_IMAGE_NAME {
            return Ok(ResolvedImageGuest {
                guest: base_guest.clone(),
                cache_image: None,
            });
        }
        let dir = self.image_dir(name);
        let manifest = read_manifest(&dir.join(IMAGE_MANIFEST))?;
        let kernel_image = dir.join(&manifest.kernel_image);
        let rootfs_image = dir.join(&manifest.rootfs_image);
        let cache_image = manifest.cache_image.as_ref().map(|path| dir.join(path));
        ensure_file(&kernel_image, "image kernel")?;
        ensure_file(&rootfs_image, "image rootfs")?;
        if let Some(path) = &cache_image {
            ensure_file(path, "image cache")?;
        }
        let mut guest = base_guest.clone();
        guest.kernel_image = kernel_image;
        guest.kernel_format =
            GuestKernelFormat::detect_from_path(&guest.kernel_image, guest.kernel_format);
        guest.rootfs_image = rootfs_image;
        Ok(ResolvedImageGuest { guest, cache_image })
    }

    pub(super) fn images_dir(&self) -> PathBuf {
        self.state_dir.join("images")
    }

    pub(super) fn image_dir(&self, name: &str) -> PathBuf {
        self.images_dir().join(name)
    }

    pub(super) fn package_cache_dir(&self) -> PathBuf {
        self.state_dir.join("package-cache")
    }
}

fn base_manifest() -> VmImageManifest {
    VmImageManifest {
        version: 1,
        name: BASE_IMAGE_NAME.into(),
        arch: default_guest_arch().unwrap_or("unknown").into(),
        alpine_version: super::types::ALPINE_VERSION.into(),
        created_at_ms: 0,
        apk: BASE_APK_PACKAGES
            .iter()
            .map(|value| (*value).into())
            .collect(),
        pip: Vec::new(),
        npm: Vec::new(),
        kernel_image: "default runtime kernel".into(),
        rootfs_image: "default runtime rootfs".into(),
        cache_image: None,
        package_count: BASE_APK_PACKAGES.len(),
    }
}

pub(super) fn read_manifest(path: &Path) -> Result<VmImageManifest> {
    let bytes = fs::read(path)
        .map_err(|error| SandboxError::io(format!("reading {}", path.display()), error))?;
    serde_json::from_slice(&bytes)
        .map_err(|error| SandboxError::json(format!("decoding {}", path.display()), error))
}

fn create_dir_all(path: &Path, context: &str) -> Result<()> {
    fs::create_dir_all(path).map_err(|error| SandboxError::io(context, error))
}

fn ensure_file(path: &Path, label: &str) -> Result<()> {
    if path.is_file() {
        return Ok(());
    }
    Err(SandboxError::not_found(format!(
        "{label} {}",
        path.display()
    )))
}
