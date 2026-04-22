use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
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
    original_default_keychain: Option<PathBuf>,
    original_search_list: Vec<PathBuf>,
}

impl TempKeychain {
    pub(super) fn create(root: &Path, settings: &DeveloperIdSettings) -> anyhow::Result<Self> {
        let dir = unique_temp_dir(root)?;
        fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
        let path = dir.join("sagens-signing.keychain-db");
        let keychain_password =
            settings.required("KEYCHAIN_PASSWORD", settings.keychain_password.as_deref())?;
        let original_default_keychain = current_default_keychain().ok();
        let original_search_list = current_search_list().unwrap_or_default();

        let configured = (|| -> anyhow::Result<(String, Option<String>)> {
            run(
                crate::cmd::tool_command("security")
                    .arg("create-keychain")
                    .arg("-p")
                    .arg(keychain_password)
                    .arg(&path),
                "creating temporary signing keychain",
            )?;
            run(
                crate::cmd::tool_command("security")
                    .arg("set-keychain-settings")
                    .arg("-lut")
                    .arg("21600")
                    .arg(&path),
                "configuring temporary signing keychain",
            )?;
            run(
                crate::cmd::tool_command("security")
                    .arg("unlock-keychain")
                    .arg("-p")
                    .arg(keychain_password)
                    .arg(&path),
                "unlocking temporary signing keychain",
            )?;
            activate_keychain(&path, &original_search_list)?;
            import_certificates(&dir, &path, settings)?;
            run(
                crate::cmd::tool_command("security")
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
            Ok((app_identity, installer_identity))
        })();

        match configured {
            Ok((app_identity, installer_identity)) => Ok(Self {
                dir,
                path,
                app_identity,
                installer_identity,
                original_default_keychain,
                original_search_list,
            }),
            Err(error) => {
                cleanup_failed_keychain(
                    &path,
                    &dir,
                    &original_default_keychain,
                    &original_search_list,
                );
                Err(error)
            }
        }
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
        restore_keychain_state(&self.original_default_keychain, &self.original_search_list);
        let _ = crate::cmd::tool_command("security")
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
        crate::cmd::tool_command("security")
            .arg("import")
            .arg(&certificate)
            .arg("-f")
            .arg("pkcs12")
            .arg("-k")
            .arg(keychain)
            .arg("-P")
            .arg(p12_password)
            .arg("-A")
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

fn activate_keychain(path: &Path, original_search_list: &[PathBuf]) -> anyhow::Result<()> {
    let mut search_list = Vec::with_capacity(original_search_list.len() + 1);
    search_list.push(path.to_path_buf());
    for existing in original_search_list {
        if existing != path {
            search_list.push(existing.clone());
        }
    }
    set_search_list(&search_list)?;
    set_default_keychain(path)
}

fn cleanup_failed_keychain(
    path: &Path,
    dir: &Path,
    original_default_keychain: &Option<PathBuf>,
    original_search_list: &[PathBuf],
) {
    restore_keychain_state(original_default_keychain, original_search_list);
    let _ = crate::cmd::tool_command("security")
        .arg("delete-keychain")
        .arg(path)
        .stdin(Stdio::null())
        .status();
    let _ = fs::remove_dir_all(dir);
}

fn restore_keychain_state(
    original_default_keychain: &Option<PathBuf>,
    original_search_list: &[PathBuf],
) {
    if let Some(path) = original_default_keychain {
        let _ = set_default_keychain(path);
    }
    if !original_search_list.is_empty() {
        let _ = set_search_list(original_search_list);
    }
}

fn current_default_keychain() -> anyhow::Result<PathBuf> {
    let output = crate::cmd::tool_command("security")
        .arg("default-keychain")
        .arg("-d")
        .arg("user")
        .output()
        .context("reading current default keychain")?;
    let stdout = String::from_utf8(output.stdout).context("decoding default-keychain output")?;
    parse_keychain_paths(&stdout)
        .into_iter()
        .next()
        .context("default-keychain output did not include a keychain path")
}

fn current_search_list() -> anyhow::Result<Vec<PathBuf>> {
    let output = crate::cmd::tool_command("security")
        .arg("list-keychains")
        .arg("-d")
        .arg("user")
        .output()
        .context("reading current keychain search list")?;
    let stdout = String::from_utf8(output.stdout).context("decoding list-keychains output")?;
    Ok(parse_keychain_paths(&stdout))
}

fn set_default_keychain(path: &Path) -> anyhow::Result<()> {
    run(
        crate::cmd::tool_command("security")
            .arg("default-keychain")
            .arg("-d")
            .arg("user")
            .arg("-s")
            .arg(path),
        "setting default signing keychain",
    )
}

fn set_search_list(paths: &[PathBuf]) -> anyhow::Result<()> {
    let mut command = crate::cmd::tool_command("security");
    command
        .arg("list-keychains")
        .arg("-d")
        .arg("user")
        .arg("-s");
    for path in paths {
        command.arg(path);
    }
    run(&mut command, "updating keychain search list")
}

fn parse_keychain_paths(stdout: &str) -> Vec<PathBuf> {
    stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| line.trim_matches('"'))
        .filter(|line| !line.is_empty())
        .map(PathBuf::from)
        .collect()
}
