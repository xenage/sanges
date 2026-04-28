use std::env;
use std::path::Path;

use anyhow::{anyhow, ensure};

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
pub(super) const SKIP_INSTALLER_ENV: &str = "SAGENS_SKIP_INSTALLER";

pub(super) enum SigningSettings {
    AdHoc,
    DeveloperId(DeveloperIdSettings),
}

impl SigningSettings {
    pub(super) fn from_env(root: &Path) -> anyhow::Result<Self> {
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

    pub(super) fn has_notary_credentials(&self) -> bool {
        self.apple_app_specific_password.is_some()
            && self.apple_id.is_some()
            && self.team_id.is_some()
    }

    pub(super) fn required<'a>(
        &self,
        key: &str,
        value: Option<&'a str>,
    ) -> anyhow::Result<&'a str> {
        value.ok_or_else(|| anyhow!("{key} must be set for this signing flow"))
    }

    pub(super) fn installer_p12_password(&self) -> anyhow::Result<&str> {
        self.installer_p12_password
            .as_deref()
            .or(self.p12_password.as_deref())
            .ok_or_else(|| anyhow!("INSTALLER_P12_PASSWORD or P12_PASSWORD must be set"))
    }
}

pub(super) fn env_truthy(key: &str) -> bool {
    matches!(
        read_env(key).as_deref(),
        Some("1" | "true" | "TRUE" | "yes" | "YES" | "on" | "ON")
    )
}

pub(super) fn parse_env_value(value: &str) -> anyhow::Result<String> {
    if value.len() >= 2
        && ((value.starts_with('"') && value.ends_with('"'))
            || (value.starts_with('\'') && value.ends_with('\'')))
    {
        return Ok(value[1..value.len() - 1].to_string());
    }
    Ok(value.to_string())
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

fn read_env(name: &str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.is_empty())
}

fn require(value: &Option<String>, message: &str) -> anyhow::Result<()> {
    ensure!(value.is_some(), "{message}");
    Ok(())
}
