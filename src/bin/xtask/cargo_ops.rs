use std::ffi::OsStr;
use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, bail, ensure};
use sha2::{Digest, Sha256};

use super::signing;
use super::types::{Profile, repo_root};

pub(super) fn cargo_build(root: &Path, profile: Profile, bins: &[&str]) -> anyhow::Result<()> {
    let mut command = crate::cmd::tool_command("cargo");
    command.arg("build");
    if let Some(flag) = profile.cargo_flag() {
        command.arg(flag);
    }
    for bin in bins {
        command.arg("--bin").arg(bin);
    }
    command.current_dir(root);
    run(command, "running cargo build")
}

pub(super) fn cargo_test(
    root: &Path,
    profile: Profile,
    e2e_binary: Option<(&str, &Path)>,
    trailing_args: &[&str],
) -> anyhow::Result<()> {
    let mut command = crate::cmd::tool_command("cargo");
    command.arg("test");
    if let Some(flag) = profile.cargo_flag() {
        command.arg(flag);
    }
    if let Some((test_name, binary)) = e2e_binary {
        command
            .env("SAGENS_RUN_E2E", "1")
            .env("SAGENS_HOST_BINARY", binary)
            .arg("--test")
            .arg(test_name);
    }
    command.args(trailing_args).current_dir(root);
    run(command, "running cargo test")
}

pub(super) fn run_shell_e2e(root: &Path, script_rel: &str, binary: &Path) -> anyhow::Result<()> {
    let script = root.join(script_rel);
    ensure!(
        script.is_file(),
        "missing shell e2e script: {}",
        script.display()
    );
    let mut command = Command::new("bash");
    command.arg(&script).arg(binary).current_dir(root);
    run(command, "running shell e2e")
}

pub(super) fn create_package(
    out_dir: &Path,
    artifact_platform: &str,
    version: &str,
    binary: &Path,
) -> anyhow::Result<()> {
    let package_name = format!("sagens-{version}-{artifact_platform}");
    let packaged_binary = out_dir.join(&package_name);
    let sha256_path = out_dir.join(format!("{package_name}.sha256"));
    if packaged_binary.exists() {
        fs::remove_file(&packaged_binary)
            .with_context(|| format!("removing {}", packaged_binary.display()))?;
    }
    if sha256_path.exists() {
        fs::remove_file(&sha256_path)
            .with_context(|| format!("removing {}", sha256_path.display()))?;
    }
    fs::copy(binary, &packaged_binary).with_context(|| {
        format!(
            "copying {} to {}",
            binary.display(),
            packaged_binary.display()
        )
    })?;
    signing::sign_binary(&repo_root()?, &packaged_binary)?;
    let digest = sha256_file(&packaged_binary)?;
    let binary_name = packaged_binary
        .file_name()
        .and_then(OsStr::to_str)
        .context("packaged binary has no file name")?;
    write_sha256(&sha256_path, &digest, binary_name)?;
    if let Some(pkg) = signing::build_macos_pkg(
        &repo_root()?,
        &package_name,
        version,
        &packaged_binary,
        out_dir,
    )? {
        let pkg_name = pkg
            .file_name()
            .and_then(OsStr::to_str)
            .context("installer package has no file name")?;
        write_sha256(
            &out_dir.join(format!("{pkg_name}.sha256")),
            &sha256_file(&pkg)?,
            pkg_name,
        )?;
    }
    if let Some(archive) =
        signing::notarize_release_archive(&repo_root()?, &package_name, &packaged_binary, out_dir)?
    {
        let archive_name = archive
            .file_name()
            .and_then(OsStr::to_str)
            .context("notarization archive has no file name")?;
        write_sha256(
            &out_dir.join(format!("{archive_name}.sha256")),
            &sha256_file(&archive)?,
            archive_name,
        )?;
    }
    println!("created {}", packaged_binary.display());
    Ok(())
}

fn sha256_file(path: &Path) -> anyhow::Result<String> {
    let mut file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .with_context(|| format!("hashing {}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex_encode(&hasher.finalize()))
}

fn write_sha256(path: &Path, digest: &str, file_name: &str) -> anyhow::Result<()> {
    fs::write(path, format!("{digest}  {file_name}\n"))
        .with_context(|| format!("writing {}", path.display()))
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(&mut output, "{byte:02x}");
    }
    output
}

pub(super) fn run(mut command: Command, description: &str) -> anyhow::Result<()> {
    command.stdin(Stdio::null());
    let status = command
        .status()
        .with_context(|| format!("{description}: {:?}", command))?;
    if !status.success() {
        bail!("{description} failed with status {status}");
    }
    Ok(())
}
