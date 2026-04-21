use std::fs::{self, File};
use std::io::{self, Read};
use std::path::Path;

use anyhow::{Context, anyhow, bail, ensure};
use flate2::read::GzDecoder;
use sequoia_openpgp as openpgp;
use sequoia_openpgp::KeyHandle;
use sequoia_openpgp::parse::Parse;
use sequoia_openpgp::parse::stream::{
    DetachedVerifierBuilder, MessageLayer, MessageStructure, VerificationHelper,
};
use sequoia_openpgp::policy::StandardPolicy;
use sha2::{Digest, Sha256};
use tar::Archive;

pub(super) fn extract_member_from_tar_gz(
    archive_path: &Path,
    member_name: &str,
    destination: &Path,
) -> anyhow::Result<()> {
    let archive =
        File::open(archive_path).with_context(|| format!("opening {}", archive_path.display()))?;
    let decoder = GzDecoder::new(archive);
    let mut archive = Archive::new(decoder);
    for entry in archive
        .entries()
        .with_context(|| format!("reading tar entries from {}", archive_path.display()))?
    {
        let mut entry =
            entry.with_context(|| format!("reading tar entry from {}", archive_path.display()))?;
        let path = entry
            .path()
            .with_context(|| format!("reading tar path from {}", archive_path.display()))?;
        if !archive_member_matches(path.as_ref(), Path::new(member_name)) {
            continue;
        }
        let mut output = File::create(destination)
            .with_context(|| format!("creating {}", destination.display()))?;
        io::copy(&mut entry, &mut output)
            .with_context(|| format!("extracting {member_name} from {}", archive_path.display()))?;
        return Ok(());
    }
    bail!(
        "missing tar member {member_name} in {}",
        archive_path.display()
    )
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
) -> anyhow::Result<()> {
    ensure!(
        cert.fingerprint().to_string() == crate::EXPECTED_SIGNING_FINGERPRINT,
        "unexpected signing cert fingerprint"
    );
    let policy = StandardPolicy::new();
    let helper = DetachedSignatureHelper { cert: cert.clone() };
    let mut verifier = DetachedVerifierBuilder::from_file(signature_path)
        .with_context(|| format!("opening detached signature {}", signature_path.display()))?
        .with_policy(&policy, None, helper)
        .context("building detached OpenPGP verifier")?;
    verifier.verify_file(payload_path).with_context(|| {
        format!(
            "verifying detached signature for {}",
            payload_path.display()
        )
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

fn archive_member_matches(actual: &Path, expected: &Path) -> bool {
    if actual == expected {
        return true;
    }
    actual
        .strip_prefix(".")
        .map(|path| path == expected)
        .unwrap_or(false)
}

struct DetachedSignatureHelper {
    cert: openpgp::Cert,
}

impl VerificationHelper for DetachedSignatureHelper {
    fn get_certs(&mut self, _: &[KeyHandle]) -> openpgp::Result<Vec<openpgp::Cert>> {
        Ok(vec![self.cert.clone()])
    }

    fn check(&mut self, structure: MessageStructure<'_>) -> openpgp::Result<()> {
        let Some(layer) = structure.into_iter().next() else {
            return Err(anyhow!("missing detached signature results"));
        };
        match layer {
            MessageLayer::SignatureGroup { results } => {
                if results.iter().any(|result| result.is_ok()) {
                    return Ok(());
                }
                Err(anyhow!("detached signature did not validate"))
            }
            _ => Err(anyhow!("unexpected OpenPGP message structure")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::archive_member_matches;
    use std::path::Path;

    #[test]
    fn matches_member_with_dot_prefix() {
        assert!(archive_member_matches(
            Path::new("./APKINDEX"),
            Path::new("APKINDEX")
        ));
        assert!(archive_member_matches(
            Path::new("./boot/vmlinuz-virt"),
            Path::new("boot/vmlinuz-virt")
        ));
    }
}
