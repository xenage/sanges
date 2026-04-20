use std::ffi::OsStr;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, bail, ensure};

pub(super) fn run(command: &mut Command, description: &str) -> anyhow::Result<()> {
    command.stdin(Stdio::null());
    let status = command
        .status()
        .with_context(|| format!("{description}: {:?}", printable(command)))?;
    if !status.success() {
        bail!("{description} failed with status {status}");
    }
    Ok(())
}

pub(super) fn find_codesign_identity(keychain: &Path) -> anyhow::Result<String> {
    find_identity(keychain, "Developer ID Application:")
}

pub(super) fn find_installer_identity(keychain: &Path) -> anyhow::Result<String> {
    find_identity(keychain, "Developer ID Installer:")
}

fn find_identity(keychain: &Path, marker: &str) -> anyhow::Result<String> {
    let output = crate::cmd::tool_command("security")
        .arg("find-identity")
        .arg("-v")
        .arg("-p")
        .arg("basic")
        .arg(keychain)
        .output()
        .context("listing codesigning identities")?;
    ensure!(
        output.status.success(),
        "security find-identity failed with status {}",
        output.status
    );
    let stdout = String::from_utf8(output.stdout).context("decoding find-identity output")?;
    for line in stdout.lines() {
        if line.contains(marker) {
            let mut parts = line.split('"');
            let _ = parts.next();
            if let Some(identity) = parts.next() {
                return Ok(identity.to_string());
            }
        }
    }
    bail!("{marker} identity not found in imported certificate")
}

fn printable(command: &Command) -> String {
    let mut parts = Vec::new();
    parts.push(command.get_program().to_string_lossy().into_owned());
    parts.extend(
        command
            .get_args()
            .map(OsStr::to_string_lossy)
            .map(|value| value.into_owned()),
    );
    parts.join(" ")
}
