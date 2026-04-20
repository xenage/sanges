use std::path::{Path, PathBuf};

use super::cargo_ops::run;

use super::runtime::{
    build_guest_artifacts, clean_runtime_dir, ensure_runtime_bundle, guest_kernel_path,
    guest_rootfs_path, maybe_init_submodules, resolve_artifacts,
};
use super::signing;
use super::types::{EMBED_MANIFEST, EmbedManifestGuard, Platform, Profile, target_root};

pub(super) struct BuiltHostBinary {
    pub(super) artifact_platform: String,
    pub(super) path: PathBuf,
}

pub(super) fn build_distribution_binary(
    root: &Path,
    profile: Profile,
    refresh_runtime: bool,
    refresh_guest: bool,
) -> anyhow::Result<BuiltHostBinary> {
    maybe_init_submodules(root)?;
    let current = Platform::current()?;
    let path = build_native_binary(root, current, profile, refresh_runtime, refresh_guest)?;
    Ok(BuiltHostBinary {
        artifact_platform: current.as_str().into(),
        path,
    })
}

fn build_native_binary(
    root: &Path,
    platform: Platform,
    profile: Profile,
    refresh_runtime: bool,
    refresh_guest: bool,
) -> anyhow::Result<PathBuf> {
    let _manifest_guard =
        prepare_embedded_assets(root, platform, profile, refresh_runtime, refresh_guest)?;
    cargo_build_host(root, profile)?;
    let binary = target_root(root).join(profile.as_str()).join("sagens");
    signing::sign_binary(root, &binary)?;
    Ok(binary)
}

fn prepare_embedded_assets(
    root: &Path,
    platform: Platform,
    profile: Profile,
    refresh_runtime: bool,
    refresh_guest: bool,
) -> anyhow::Result<EmbedManifestGuard> {
    if refresh_runtime {
        clean_runtime_dir(root, platform)?;
    }
    let runtime = ensure_runtime_bundle(root, platform)?;
    if refresh_guest || missing_guest_artifacts(root, platform) {
        build_guest_artifacts(root, platform, profile)?;
    }
    let artifacts = resolve_artifacts(root, platform, runtime)?;
    let manifest_path = root.join(EMBED_MANIFEST);
    EmbedManifestGuard::write(&manifest_path, &artifacts)
}

fn missing_guest_artifacts(root: &Path, platform: Platform) -> bool {
    !guest_kernel_path(root, platform).is_file() || !guest_rootfs_path(root, platform).is_file()
}

fn cargo_build_host(root: &Path, profile: Profile) -> anyhow::Result<()> {
    let mut command = crate::cmd::tool_command("cargo");
    command.arg("build");
    if let Some(flag) = profile.cargo_flag() {
        command.arg(flag);
    }
    command.arg("--bin").arg("sagens");
    command.current_dir(root);
    run(command, "building sagens host binary")
}
