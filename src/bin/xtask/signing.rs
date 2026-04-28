#[path = "signing/command.rs"]
mod command;
#[path = "signing/installer.rs"]
mod installer;
#[path = "signing/keychain.rs"]
mod keychain;
#[path = "signing/notary.rs"]
mod notary;
#[path = "signing/python.rs"]
mod python;
#[path = "signing/settings.rs"]
mod settings;

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, anyhow, bail};

use self::command::run;
use self::installer::{
    build_signed_pkg, build_unsigned_pkg, installer_pkg_version, installer_stage_dir, staple_ticket,
};
use self::keychain::TempKeychain;
use self::notary::notarize_path;
use self::python::stage_python_binary;
use self::settings::{
    DeveloperIdSettings, SKIP_INSTALLER_ENV, SigningSettings, env_truthy, parse_env_value,
};

pub fn load_repo_env(root: &Path) -> anyhow::Result<()> {
    let path = root.join(".env");
    if !path.is_file() {
        return Ok(());
    }
    let contents =
        fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    for (index, raw_line) in contents.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| anyhow!("invalid .env line {}: expected KEY=VALUE", index + 1))?;
        let key = key.trim();
        if key.is_empty() {
            bail!("invalid .env line {}: empty key", index + 1);
        }
        if env::var_os(key).is_some() {
            continue;
        }
        unsafe {
            env::set_var(key, parse_env_value(value.trim())?);
        }
    }
    Ok(())
}

pub fn sign_binary(root: &Path, binary: &Path) -> anyhow::Result<()> {
    if env::consts::OS != "macos" {
        return Ok(());
    }
    let entitlements = root.join("macos").join("sagens.entitlements");
    sign_macos_code(root, binary, Some(&entitlements), "sagens binary")
}

pub fn sign_native_payload(root: &Path, path: &Path, host: bool) -> anyhow::Result<()> {
    if env::consts::OS != "macos" {
        return Ok(());
    }
    let entitlements = host.then(|| root.join("macos").join("sagens.entitlements"));
    sign_macos_code(
        root,
        path,
        entitlements.as_deref(),
        if host {
            "sagens host payload"
        } else {
            "Python native payload"
        },
    )
}

fn sign_macos_code(
    root: &Path,
    path: &Path,
    entitlements: Option<&Path>,
    description: &str,
) -> anyhow::Result<()> {
    match SigningSettings::from_env(root)? {
        SigningSettings::AdHoc => ad_hoc_codesign(entitlements, path, description),
        SigningSettings::DeveloperId(settings) => {
            let keychain = TempKeychain::create(root, &settings)?;
            developer_id_codesign(
                keychain.identity(),
                keychain.path(),
                entitlements,
                path,
                description,
            )
        }
    }
}

pub fn build_macos_pkg(
    root: &Path,
    package_name: &str,
    version: &str,
    binary: &Path,
    out_dir: &Path,
) -> anyhow::Result<Option<PathBuf>> {
    if env::consts::OS != "macos" || env_truthy(SKIP_INSTALLER_ENV) {
        return Ok(None);
    }
    let pkg = out_dir.join(format!("{package_name}.pkg"));
    if pkg.exists() {
        fs::remove_file(&pkg).with_context(|| format!("removing {}", pkg.display()))?;
    }
    let stage = installer_stage_dir(root, package_name);
    if stage.exists() {
        fs::remove_dir_all(&stage).with_context(|| format!("removing {}", stage.display()))?;
    }
    let install_root = stage.join("root");
    let install_bin_dir = install_root.join("usr").join("local").join("bin");
    fs::create_dir_all(&install_bin_dir)
        .with_context(|| format!("creating {}", install_bin_dir.display()))?;
    let staged_binary = install_bin_dir.join("sagens");
    fs::copy(binary, &staged_binary).with_context(|| {
        format!(
            "copying {} to {}",
            binary.display(),
            staged_binary.display()
        )
    })?;
    let version = installer_pkg_version(version);
    let settings = SigningSettings::from_env(root)?;
    match settings {
        SigningSettings::AdHoc => build_unsigned_pkg(version, &install_root, &pkg)?,
        SigningSettings::DeveloperId(settings) => {
            let keychain = TempKeychain::create(root, &settings)?;
            if let Some(identity) = keychain.installer_identity() {
                build_signed_pkg(version, &install_root, &pkg, identity, keychain.path())?;
                if settings.has_notary_credentials() {
                    notarize_path(&pkg, &settings)?;
                    staple_ticket(&pkg)?;
                }
            } else {
                build_unsigned_pkg(version, &install_root, &pkg)?;
            }
        }
    }
    fs::remove_dir_all(&stage).with_context(|| format!("removing {}", stage.display()))?;
    Ok(Some(pkg))
}

pub fn notarize_release_archive(
    root: &Path,
    package_name: &str,
    binary: &Path,
    out_dir: &Path,
) -> anyhow::Result<Option<PathBuf>> {
    if env::consts::OS != "macos" {
        return Ok(None);
    }
    let settings = match SigningSettings::from_env(root)? {
        SigningSettings::AdHoc => return Ok(None),
        SigningSettings::DeveloperId(settings) => settings,
    };
    if !settings.has_notary_credentials() {
        return Ok(None);
    }
    let archive = out_dir.join(format!("{package_name}.zip"));
    if archive.exists() {
        fs::remove_file(&archive).with_context(|| format!("removing {}", archive.display()))?;
    }
    run(
        crate::cmd::tool_command("ditto")
            .arg("-c")
            .arg("-k")
            .arg("--keepParent")
            .arg(binary)
            .arg(&archive),
        "creating notarization archive",
    )?;
    notarize_path(&archive, &settings)?;
    Ok(Some(archive))
}

pub fn stage_python_binary_payload(package_root: &Path, binary: &Path) -> anyhow::Result<PathBuf> {
    stage_python_binary(package_root, binary)
}

fn ad_hoc_codesign(
    entitlements: Option<&Path>,
    path: &Path,
    description: &str,
) -> anyhow::Result<()> {
    let mut command = crate::cmd::tool_command("codesign");
    command.arg("--force").arg("--sign").arg("-");
    if let Some(entitlements) = entitlements {
        command.arg("--entitlements").arg(entitlements);
    }
    command.arg("--timestamp=none").arg(path);
    run(&mut command, &format!("ad-hoc codesigning {description}"))
}

fn developer_id_codesign(
    identity: &str,
    keychain: &Path,
    entitlements: Option<&Path>,
    path: &Path,
    description: &str,
) -> anyhow::Result<()> {
    let mut command = crate::cmd::tool_command("codesign");
    command
        .arg("--force")
        .arg("--sign")
        .arg(identity)
        .arg("--keychain")
        .arg(keychain);
    if let Some(entitlements) = entitlements {
        command.arg("--entitlements").arg(entitlements);
    }
    command
        .arg("--options")
        .arg("runtime")
        .arg("--timestamp")
        .arg(path);
    run(
        &mut command,
        &format!("Developer ID codesigning {description}"),
    )
}
