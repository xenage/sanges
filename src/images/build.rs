use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, bail, ensure};
use uuid::Uuid;

use crate::{Result, SandboxError};

use super::alpine::{
    alpine_signing_cert, fetch_indexes, fetch_rootfs, http_client, rootfs_tar_name,
};
use super::ext4::build_ext4_image;
use super::packages::{download_packages, load_indexes, resolve_packages, wanted_apk_packages};
use super::rootfs::{
    extract_guest_agent, prepare_cache_dir, prepare_npm_cache, prepare_pip_cache, rebuild_rootfs,
};
use super::store::ImageStore;
use super::types::{
    ALPINE_VERSION, BASE_IMAGE_NAME, CACHE_IMAGE_MIB, DEFAULT_IMAGE_MIB, IMAGE_MANIFEST,
    ImageBuildSpec, VmImageManifest, default_guest_arch, validate_image_name,
};

pub(super) fn build_image(spec: ImageBuildSpec) -> Result<VmImageManifest> {
    let name = spec.name.clone();
    build_image_inner(spec)
        .map_err(|error| SandboxError::backend(format!("building image {name}: {error:#}")))
}

fn build_image_inner(spec: ImageBuildSpec) -> anyhow::Result<VmImageManifest> {
    validate_image_name(&spec.name).map_err(|error| anyhow::anyhow!("{error}"))?;
    ensure!(spec.name != BASE_IMAGE_NAME, "base image is reserved");
    let store = ImageStore::new(&spec.state_dir);
    store
        .ensure_layout()
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    let arch = default_guest_arch()
        .map_err(|error| anyhow::anyhow!("{error}"))?
        .to_owned();
    let image_dir = store.image_dir(&spec.name);
    if image_dir.exists() {
        if !spec.force_refresh {
            bail!(
                "image {} already exists; pass --force-refresh to rebuild it",
                spec.name
            );
        }
        fs::remove_dir_all(&image_dir)
            .with_context(|| format!("removing {}", image_dir.display()))?;
    }
    let build_dir =
        store
            .images_dir()
            .join(format!(".build-{}-{}", spec.name, Uuid::new_v4().simple()));
    let downloads = store
        .package_cache_dir()
        .join("alpine")
        .join("v3.23")
        .join(&arch);
    let apk_dir = downloads.join("apk");
    let index_dir = downloads.join("index");
    let rootfs_dir = downloads.join("rootfs");
    let work_rootfs = build_dir.join("rootfs");
    let work_cache = build_dir.join("cache");
    let guest_agent = build_dir.join("sagens-guest-agent");
    fs::create_dir_all(&image_dir)?;
    fs::create_dir_all(&build_dir)?;
    fs::create_dir_all(&apk_dir)?;
    fs::create_dir_all(&index_dir)?;
    fs::create_dir_all(&rootfs_dir)?;

    let client = http_client()?;
    let signing_cert = alpine_signing_cert(&client)?;
    let rootfs_tar_name = rootfs_tar_name(&arch);
    let rootfs_tar = rootfs_dir.join(&rootfs_tar_name);
    fetch_rootfs(
        &client,
        &signing_cert,
        &arch,
        &rootfs_tar,
        &rootfs_tar_name,
        spec.force_refresh,
    )?;
    fetch_indexes(&client, &index_dir, &arch, spec.force_refresh)?;
    let (packages, providers) = load_indexes(&index_dir)?;
    let wanted = wanted_apk_packages(&spec.apk, !spec.npm.is_empty());
    let resolved = resolve_packages(&packages, &providers, &wanted)?;
    download_packages(&client, &apk_dir, &arch, &resolved, spec.force_refresh)?;
    extract_guest_agent(&spec.base_guest.rootfs_image, &guest_agent)?;
    rebuild_rootfs(
        &rootfs_tar,
        &apk_dir,
        &resolved,
        &work_rootfs,
        &guest_agent,
        !spec.pip.is_empty(),
        !spec.npm.is_empty(),
    )?;

    prepare_cache_dir(&work_cache, &apk_dir, &resolved)?;
    prepare_pip_cache(&work_cache, &arch, &spec.pip)?;
    prepare_npm_cache(&work_cache, &spec.npm)?;
    write_cache_metadata(&work_cache, &spec)?;

    fs::copy(
        &spec.base_guest.kernel_image,
        image_dir.join("vmlinuz-virt"),
    )
    .with_context(|| {
        format!(
            "copying base kernel {}",
            spec.base_guest.kernel_image.display()
        )
    })?;
    build_ext4_image(
        &work_rootfs,
        &image_dir.join("rootfs.raw"),
        spec.min_image_mib.max(DEFAULT_IMAGE_MIB),
    )?;
    build_ext4_image(&work_cache, &image_dir.join("cache.raw"), CACHE_IMAGE_MIB)?;

    let manifest = VmImageManifest {
        version: 1,
        name: spec.name,
        arch,
        alpine_version: ALPINE_VERSION.into(),
        created_at_ms: now_ms(),
        apk: spec.apk,
        pip: spec.pip,
        npm: spec.npm,
        kernel_image: "vmlinuz-virt".into(),
        rootfs_image: "rootfs.raw".into(),
        cache_image: Some("cache.raw".into()),
        package_count: resolved.len(),
    };
    fs::write(
        image_dir.join(IMAGE_MANIFEST),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    let _ = fs::remove_dir_all(&build_dir);
    Ok(manifest)
}

fn write_cache_metadata(cache_dir: &std::path::Path, spec: &ImageBuildSpec) -> anyhow::Result<()> {
    fs::write(
        cache_dir.join("image.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "name": spec.name,
            "apk": spec.apk,
            "pip": spec.pip,
            "npm": spec.npm,
            "alpine_version": ALPINE_VERSION,
        }))?,
    )?;
    Ok(())
}

fn now_ms() -> u64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_millis() as u64,
        Err(_) => 0,
    }
}
