use std::env;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, bail, ensure};
use sequoia_openpgp as openpgp;
use sha2::{Digest, Sha256};

pub(super) fn extract_member_from_tar_gz(
    archive_path: &Path,
    member_name: &str,
    destination: &Path,
) -> anyhow::Result<()> {
    let tar = find_in_path("tar").context("tar is required to extract Alpine archive members")?;
    let output = Command::new(&tar)
        .arg("-x")
        .arg("-O")
        .arg("-z")
        .arg("-f")
        .arg(archive_path)
        .arg(member_name)
        .output()
        .with_context(|| format!("running {} to extract {member_name}", tar.display()))?;
    ensure!(
        output.status.success(),
        "missing tar member {member_name} in {}",
        archive_path.display()
    );
    fs::write(destination, output.stdout)
        .with_context(|| format!("writing {}", destination.display()))?;
    Ok(())
}

pub(super) fn verify_sha256(payload_path: &Path, checksum_path: &Path) -> anyhow::Result<()> {
    let checksum_file = fs::read_to_string(checksum_path)
        .with_context(|| format!("reading {}", checksum_path.display()))?;
    let (expected_hex, file_name) = checksum_file
        .split_once("  ")
        .context("invalid .sha256 format")?;
    ensure!(
        file_name.trim()
            == payload_path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default(),
        "checksum file name does not match payload name"
    );
    let mut hasher = Sha256::new();
    let mut file =
        File::open(payload_path).with_context(|| format!("opening {}", payload_path.display()))?;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let expected = decode_hex(expected_hex.trim())?;
    let actual = hasher.finalize();
    ensure!(
        expected.as_slice() == &actual[..],
        "sha256 mismatch for {}",
        payload_path.display()
    );
    Ok(())
}

pub(super) fn verify_detached_signature(
    payload_path: &Path,
    signature_path: &Path,
    cert: &openpgp::Cert,
    signing_key: &str,
) -> anyhow::Result<()> {
    ensure!(
        cert.fingerprint().to_string() == crate::EXPECTED_SIGNING_FINGERPRINT,
        "unexpected signing cert fingerprint"
    );
    let gpg = find_in_path("gpg").context("gpg is required to dearmor Alpine signing keys")?;
    let gpgv =
        find_in_path("gpgv").context("gpgv is required to verify Alpine detached signatures")?;
    let temp_home = env::temp_dir().join(format!(
        "agent-box-gpg-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("reading system clock")?
            .as_nanos()
    ));
    fs::create_dir_all(&temp_home)?;
    let key_path = temp_home.join("signing-key.asc");
    fs::write(&key_path, signing_key)?;
    let keyring_path = temp_home.join("signing-key.gpg");
    let dearmor_status = Command::new(&gpg)
        .arg("--batch")
        .arg("--quiet")
        .arg("--yes")
        .arg("--dearmor")
        .arg("--output")
        .arg(&keyring_path)
        .arg(&key_path)
        .status()
        .with_context(|| format!("running {} --dearmor", gpg.display()))?;
    ensure!(dearmor_status.success(), "gpg key dearmor failed");

    let verify_status = Command::new(&gpgv)
        .arg("--keyring")
        .arg(&keyring_path)
        .arg(signature_path)
        .arg(payload_path)
        .status()
        .with_context(|| format!("running {} --verify", gpgv.display()))?;
    let cleanup_result = fs::remove_dir_all(&temp_home);
    ensure!(
        verify_status.success(),
        "gpgv signature verification failed"
    );
    cleanup_result.ok();
    Ok(())
}

fn find_in_path(binary: &str) -> Option<PathBuf> {
    env::var_os("PATH").and_then(|paths| {
        env::split_paths(&paths)
            .map(|dir| dir.join(binary))
            .find(|candidate| candidate.exists())
    })
}

fn decode_hex(input: &str) -> anyhow::Result<Vec<u8>> {
    ensure!(input.len().is_multiple_of(2), "hex string has odd length");
    let mut bytes = Vec::with_capacity(input.len() / 2);
    for chunk in input.as_bytes().chunks_exact(2) {
        bytes.push((decode_hex_nibble(chunk[0])? << 4) | decode_hex_nibble(chunk[1])?);
    }
    Ok(bytes)
}

fn decode_hex_nibble(byte: u8) -> anyhow::Result<u8> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => bail!("invalid hex byte {}", byte as char),
    }
}
