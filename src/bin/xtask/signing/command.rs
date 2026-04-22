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
        .arg("codesigning")
        .arg(keychain)
        .output()
        .context("listing codesigning identities")?;
    ensure!(
        output.status.success(),
        "security find-identity failed with status {}",
        output.status
    );
    let stdout = String::from_utf8(output.stdout).context("decoding find-identity output")?;
    parse_identity_hash(&stdout, marker)
}

fn parse_identity_hash(stdout: &str, marker: &str) -> anyhow::Result<String> {
    for line in stdout.lines() {
        if !line.contains(marker) {
            continue;
        }
        let mut parts = line.split_whitespace();
        let _ = parts.next();
        if let Some(hash) = parts.next() {
            return Ok(hash.to_string());
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

#[cfg(test)]
mod tests {
    use super::parse_identity_hash;

    #[test]
    fn parses_codesign_identity_hash() {
        let stdout = r#"
  1) 0123456789ABCDEF0123456789ABCDEF01234567 "Developer ID Application: Example Corp (ABCDE12345)"
  2) FEDCBA9876543210FEDCBA9876543210FEDCBA98 "Apple Development: Example Corp (ABCDE12345)"
     2 valid identities found
"#;
        let identity = parse_identity_hash(stdout, "Developer ID Application:")
            .expect("should parse application identity hash");
        assert_eq!(identity, "0123456789ABCDEF0123456789ABCDEF01234567");
    }

    #[test]
    fn errors_when_identity_is_missing() {
        let stdout = r#"
  1) FEDCBA9876543210FEDCBA9876543210FEDCBA98 "Apple Development: Example Corp (ABCDE12345)"
     1 valid identities found
"#;
        let error = parse_identity_hash(stdout, "Developer ID Application:")
            .expect_err("missing Developer ID Application identity should fail");
        assert!(
            error
                .to_string()
                .contains("Developer ID Application: identity not found"),
            "unexpected error: {error}"
        );
    }
}
