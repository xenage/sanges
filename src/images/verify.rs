use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

use anyhow::{Context, bail, ensure};
use sequoia_openpgp as openpgp;
use sequoia_openpgp::parse::Parse;
use sha2::{Digest, Sha256};

use super::types::EXPECTED_SIGNING_FINGERPRINT;

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
    let expected = decode_hex(expected_hex.trim())?;
    let actual = file_sha256(payload_path)?;
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
        cert.fingerprint().to_string() == EXPECTED_SIGNING_FINGERPRINT,
        "unexpected signing cert fingerprint"
    );
    let policy = sequoia_openpgp::policy::StandardPolicy::new();
    let helper = DetachedSignatureHelper { cert: cert.clone() };
    let mut verifier =
        sequoia_openpgp::parse::stream::DetachedVerifierBuilder::from_file(signature_path)
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

pub(super) fn hex_sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn file_sha256(path: &Path) -> anyhow::Result<Vec<u8>> {
    let mut hasher = Sha256::new();
    let mut file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finalize().to_vec())
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

struct DetachedSignatureHelper {
    cert: openpgp::Cert,
}

impl sequoia_openpgp::parse::stream::VerificationHelper for DetachedSignatureHelper {
    fn get_certs(
        &mut self,
        _: &[sequoia_openpgp::KeyHandle],
    ) -> openpgp::Result<Vec<openpgp::Cert>> {
        Ok(vec![self.cert.clone()])
    }

    fn check(
        &mut self,
        structure: sequoia_openpgp::parse::stream::MessageStructure<'_>,
    ) -> openpgp::Result<()> {
        let Some(layer) = structure.into_iter().next() else {
            return Err(anyhow::anyhow!("missing detached signature results"));
        };
        match layer {
            sequoia_openpgp::parse::stream::MessageLayer::SignatureGroup { results } => {
                if results.iter().any(|result| result.is_ok()) {
                    return Ok(());
                }
                Err(anyhow::anyhow!("detached signature did not validate"))
            }
            _ => Err(anyhow::anyhow!("unexpected OpenPGP message structure")),
        }
    }
}
