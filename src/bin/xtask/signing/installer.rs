use std::path::{Path, PathBuf};

use super::super::types::target_root;
use super::command::run;

pub(super) fn installer_stage_dir(root: &Path, package_name: &str) -> PathBuf {
    target_root(root).join("xtask-pkg").join(package_name)
}

pub(super) fn installer_pkg_version(version: &str) -> String {
    let parts = version
        .trim_start_matches('v')
        .split(|ch: char| !ch.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    if parts.is_empty() {
        "0".into()
    } else {
        parts.join(".")
    }
}

pub(super) fn build_unsigned_pkg(
    version: String,
    install_root: &Path,
    pkg: &Path,
) -> anyhow::Result<()> {
    run(
        crate::cmd::tool_command("pkgbuild")
            .arg("--root")
            .arg(install_root)
            .arg("--identifier")
            .arg("dev.tvorogme.sagens")
            .arg("--version")
            .arg(version)
            .arg("--install-location")
            .arg("/")
            .arg("--ownership")
            .arg("recommended")
            .arg(pkg),
        "building macOS installer package",
    )
}

pub(super) fn build_signed_pkg(
    version: String,
    install_root: &Path,
    pkg: &Path,
    identity: &str,
    keychain: &Path,
) -> anyhow::Result<()> {
    run(
        crate::cmd::tool_command("pkgbuild")
            .arg("--root")
            .arg(install_root)
            .arg("--identifier")
            .arg("dev.tvorogme.sagens")
            .arg("--version")
            .arg(version)
            .arg("--install-location")
            .arg("/")
            .arg("--ownership")
            .arg("recommended")
            .arg("--sign")
            .arg(identity)
            .arg("--keychain")
            .arg(keychain)
            .arg(pkg),
        "building signed macOS installer package",
    )
}

pub(super) fn staple_ticket(path: &Path) -> anyhow::Result<()> {
    run(
        crate::cmd::tool_command("xcrun")
            .arg("stapler")
            .arg("staple")
            .arg(path),
        "stapling notarization ticket",
    )
}
