use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;

use super::super::types::target_root;
use super::DeveloperIdSettings;
use super::command::{find_codesign_identity, find_installer_identity, run};

pub(super) struct TempKeychain {
    dir: PathBuf,
    path: PathBuf,
    app_identity: String,
    installer_identity: Option<String>,
}

impl TempKeychain {
    pub(super) fn create(root: &Path, settings: &DeveloperIdSettings) -> anyhow::Result<Self> {
        let dir = unique_temp_dir(root)?;
        fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
        let path = dir.join("sagens-signing.keychain-db");
        let keychain_password =
            settings.required("KEYCHAIN_PASSWORD", settings.keychain_password.as_deref())?;

        run(
            Command::new("rtk")
                .arg("security")
                .arg("create-keychain")
                .arg("-p")
                .arg(keychain_password)
                .arg(&path),
            "creating temporary signing keychain",
        )?;
        run(
            Command::new("rtk")
                .arg("security")
                .arg("set-keychain-settings")
                .arg("-lut")
                .arg("21600")
                .arg(&path),
            "configuring temporary signing keychain",
        )?;
        run(
            Command::new("rtk")
                .arg("security")
                .arg("unlock-keychain")
                .arg("-p")
                .arg(keychain_password)
                .arg(&path),
            "unlocking temporary signing keychain",
        )?;
        import_certificates(&dir, &path, settings)?;
        run(
            Command::new("rtk")
                .arg("security")
                .arg("set-key-partition-list")
                .arg("-S")
                .arg("apple-tool:,apple:,codesign:,productbuild:,productsign:")
                .arg("-s")
                .arg("-k")
                .arg(keychain_password)
                .arg(&path),
            "configuring keychain access for codesign",
        )?;
        let app_identity = find_codesign_identity(&path)?;
        let installer_identity = find_installer_identity(&path).ok();
        Ok(Self {
            dir,
            path,
            app_identity,
            installer_identity,
        })
    }

    pub(super) fn identity(&self) -> &str {
        &self.app_identity
    }

    pub(super) fn installer_identity(&self) -> Option<&str> {
        self.installer_identity.as_deref()
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempKeychain {
    fn drop(&mut self) {
        let _ = Command::new("rtk")
            .arg("security")
            .arg("delete-keychain")
            .arg(&self.path)
            .stdin(Stdio::null())
            .status();
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn unique_temp_dir(root: &Path) -> anyhow::Result<PathBuf> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("reading system clock for temp dir")?
        .as_nanos();
    Ok(target_root(root)
        .join("xtask-signing")
        .join(format!("{}-{now}", std::process::id())))
}

fn import_certificates(
    dir: &Path,
    keychain: &Path,
    settings: &DeveloperIdSettings,
) -> anyhow::Result<()> {
    import_certificate(
        dir.join("certificate-app.p12"),
        keychain,
        settings.required(
            "BUILD_CERTIFICATE_BASE64",
            settings.build_certificate_base64.as_deref(),
        )?,
        settings.required("P12_PASSWORD", settings.p12_password.as_deref())?,
        "Developer ID Application certificate",
    )?;
    if let Some(certificate_base64) = settings.installer_certificate_base64.as_deref() {
        import_certificate(
            dir.join("certificate-installer.p12"),
            keychain,
            certificate_base64,
            settings.installer_p12_password()?,
            "Developer ID Installer certificate",
        )?;
    }
    Ok(())
}

fn import_certificate(
    certificate: PathBuf,
    keychain: &Path,
    certificate_base64: &str,
    p12_password: &str,
    label: &str,
) -> anyhow::Result<()> {
    fs::write(&certificate, decode_base64(certificate_base64)?)
        .with_context(|| format!("writing {}", certificate.display()))?;
    run(
        Command::new("rtk")
            .arg("security")
            .arg("import")
            .arg(&certificate)
            .arg("-k")
            .arg(keychain)
            .arg("-P")
            .arg(p12_password)
            .arg("-T")
            .arg("/usr/bin/codesign")
            .arg("-T")
            .arg("/usr/bin/productbuild")
            .arg("-T")
            .arg("/usr/bin/productsign")
            .arg("-T")
            .arg("/usr/bin/security"),
        &format!("importing {label}"),
    )
}

fn decode_base64(value: &str) -> anyhow::Result<Vec<u8>> {
    let mut output = Vec::new();
    let mut buffer: u32 = 0;
    let mut bits = 0u8;
    for byte in value.bytes() {
        let digit = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' | b'\n' | b'\r' | b'\t' | b' ' => continue,
            _ => anyhow::bail!("BUILD_CERTIFICATE_BASE64 contains invalid base64 data"),
        } as u32;
        buffer = (buffer << 6) | digit;
        bits += 6;
        while bits >= 8 {
            bits -= 8;
            output.push(((buffer >> bits) & 0xff) as u8);
            buffer &= (1 << bits) - 1;
        }
    }
    Ok(output)
}
