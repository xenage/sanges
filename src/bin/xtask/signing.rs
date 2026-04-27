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

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, anyhow, bail, ensure};

use self::command::run;
use self::installer::{
    build_signed_pkg, build_unsigned_pkg, installer_pkg_version, installer_stage_dir, staple_ticket,
};
use self::keychain::TempKeychain;
use self::notary::notarize_path;
use self::python::stage_python_binary;

const ENV_KEYS: &[&str] = &[
    "APPLE_APP_SPECIFIC_PASSWORD",
    "APPLE_ID",
    "TEAM_ID",
    "BUILD_CERTIFICATE_BASE64",
    "INSTALLER_CERTIFICATE_BASE64",
    "KEYCHAIN_PASSWORD",
    "P12_PASSWORD",
    "INSTALLER_P12_PASSWORD",
];
const FORCE_SIGN_ENV: &str = "SAGENS_FORCE_SIGN";
const SKIP_INSTALLER_ENV: &str = "SAGENS_SKIP_INSTALLER";

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

enum SigningSettings {
    AdHoc,
    DeveloperId(DeveloperIdSettings),
}

impl SigningSettings {
    fn from_env(root: &Path) -> anyhow::Result<Self> {
        let env = DeveloperIdSettings::read_env();
        let force_sign = env_truthy(FORCE_SIGN_ENV);
        if force_sign {
            ensure!(
                !env.is_empty(),
                "Developer ID signing was requested, but signing secrets are missing"
            );
            env.validate()?;
            return Ok(Self::DeveloperId(env));
        }
        if env.is_empty() || !release_signing_allowed(root) {
            return Ok(Self::AdHoc);
        }
        env.validate()?;
        Ok(Self::DeveloperId(env))
    }
}

pub(super) struct DeveloperIdSettings {
    pub(super) apple_app_specific_password: Option<String>,
    pub(super) apple_id: Option<String>,
    pub(super) team_id: Option<String>,
    pub(super) build_certificate_base64: Option<String>,
    pub(super) installer_certificate_base64: Option<String>,
    pub(super) keychain_password: Option<String>,
    pub(super) p12_password: Option<String>,
    pub(super) installer_p12_password: Option<String>,
}

impl DeveloperIdSettings {
    fn read_env() -> Self {
        Self {
            apple_app_specific_password: read_env("APPLE_APP_SPECIFIC_PASSWORD"),
            apple_id: read_env("APPLE_ID"),
            team_id: read_env("TEAM_ID"),
            build_certificate_base64: read_env("BUILD_CERTIFICATE_BASE64"),
            installer_certificate_base64: read_env("INSTALLER_CERTIFICATE_BASE64"),
            keychain_password: read_env("KEYCHAIN_PASSWORD"),
            p12_password: read_env("P12_PASSWORD"),
            installer_p12_password: read_env("INSTALLER_P12_PASSWORD"),
        }
    }

    fn is_empty(&self) -> bool {
        ENV_KEYS
            .iter()
            .all(|key| env::var_os(key).is_none_or(|value| value.is_empty()))
    }

    fn validate(&self) -> anyhow::Result<()> {
        require(
            &self.build_certificate_base64,
            "BUILD_CERTIFICATE_BASE64 is required for Developer ID signing",
        )?;
        require(
            &self.keychain_password,
            "KEYCHAIN_PASSWORD is required for Developer ID signing",
        )?;
        require(
            &self.p12_password,
            "P12_PASSWORD is required for Developer ID signing",
        )?;
        let notary_present = [
            self.apple_app_specific_password.as_ref(),
            self.apple_id.as_ref(),
            self.team_id.as_ref(),
        ]
        .iter()
        .filter(|value| value.is_some())
        .count();
        ensure!(
            notary_present == 0 || notary_present == 3,
            "APPLE_APP_SPECIFIC_PASSWORD, APPLE_ID, and TEAM_ID must be provided together for notarization"
        );
        ensure!(
            self.installer_certificate_base64.is_some() == self.installer_p12_password.is_some()
                || self.installer_certificate_base64.is_none()
                || self.p12_password.is_some(),
            "INSTALLER_CERTIFICATE_BASE64 requires INSTALLER_P12_PASSWORD or P12_PASSWORD"
        );
        Ok(())
    }

    fn has_notary_credentials(&self) -> bool {
        self.apple_app_specific_password.is_some()
            && self.apple_id.is_some()
            && self.team_id.is_some()
    }

    fn required<'a>(&self, key: &str, value: Option<&'a str>) -> anyhow::Result<&'a str> {
        value.ok_or_else(|| anyhow!("{key} must be set for this signing flow"))
    }

    pub(super) fn installer_p12_password(&self) -> anyhow::Result<&str> {
        self.installer_p12_password
            .as_deref()
            .or(self.p12_password.as_deref())
            .ok_or_else(|| anyhow!("INSTALLER_P12_PASSWORD or P12_PASSWORD must be set"))
    }
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

fn release_signing_allowed(root: &Path) -> bool {
    if env_truthy(FORCE_SIGN_ENV) {
        return true;
    }
    if let Some(github_ref) = read_env("GITHUB_REF") {
        return github_ref == "refs/heads/main" || github_ref.starts_with("refs/tags/");
    }
    if !root.join(".git").exists() {
        return false;
    }
    git_head_has_tag(root) || git_current_branch(root).as_deref() == Some("main")
}

fn env_truthy(key: &str) -> bool {
    matches!(
        read_env(key).as_deref(),
        Some("1" | "true" | "TRUE" | "yes" | "YES" | "on" | "ON")
    )
}

fn git_current_branch(root: &Path) -> Option<String> {
    git_output(root, ["branch", "--show-current"])
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn git_head_has_tag(root: &Path) -> bool {
    git_output(root, ["tag", "--points-at", "HEAD"])
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
}

fn git_output<const N: usize>(root: &Path, args: [&str; N]) -> Option<String> {
    let output = crate::cmd::tool_command("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
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

fn read_env(name: &str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.is_empty())
}

fn parse_env_value(value: &str) -> anyhow::Result<String> {
    if value.len() >= 2
        && ((value.starts_with('"') && value.ends_with('"'))
            || (value.starts_with('\'') && value.ends_with('\'')))
    {
        return Ok(value[1..value.len() - 1].to_string());
    }
    Ok(value.to_string())
}

fn require(value: &Option<String>, message: &str) -> anyhow::Result<()> {
    ensure!(value.is_some(), "{message}");
    Ok(())
}
