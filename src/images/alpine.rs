use std::fs;
use std::path::Path;
use std::time::Duration;

use anyhow::Context;
use reqwest::blocking::Client;
use sequoia_openpgp as openpgp;
use sequoia_openpgp::cert::CertParser;
use sequoia_openpgp::parse::Parse;

use super::archive::extract_member_from_tar_gz;
use super::types::{
    ALPINE_RELEASE_KEY_URL, ALPINE_VERSION, BASE_URL, EXPECTED_SIGNING_FINGERPRINT,
};
use super::verify::{hex_sha256, verify_detached_signature, verify_sha256};

pub(super) fn http_client() -> anyhow::Result<Client> {
    Client::builder()
        .user_agent("agent-box/image-build")
        .timeout(Duration::from_secs(300))
        .build()
        .context("building HTTP client")
}

pub(super) fn alpine_signing_cert(client: &Client) -> anyhow::Result<openpgp::Cert> {
    let signing_key = client
        .get(ALPINE_RELEASE_KEY_URL)
        .send()
        .context("requesting Alpine release signing key")?
        .error_for_status()
        .context("downloading Alpine release signing key")?
        .text()
        .context("reading Alpine release signing key")?;
    CertParser::from_bytes(signing_key.as_bytes())
        .context("parsing Alpine release signing keyring")?
        .find_map(|item| match item {
            Ok(cert) if cert.fingerprint().to_string() == EXPECTED_SIGNING_FINGERPRINT => {
                Some(Ok(cert))
            }
            Ok(_) => None,
            Err(error) => Some(Err(error)),
        })
        .transpose()?
        .context("expected Alpine release signing cert not found")
}

pub(super) fn rootfs_tar_name(arch: &str) -> String {
    format!("alpine-minirootfs-{ALPINE_VERSION}-{arch}.tar.gz")
}

pub(super) fn fetch_rootfs(
    client: &Client,
    signing_cert: &openpgp::Cert,
    arch: &str,
    destination: &Path,
    tar_name: &str,
    force_refresh: bool,
) -> anyhow::Result<()> {
    let url = format!("{BASE_URL}/releases/{arch}/{tar_name}");
    let checksum_path = destination.with_file_name(format!("{tar_name}.sha256"));
    let signature_path = destination.with_file_name(format!("{tar_name}.asc"));
    fetch_to_file(client, &url, destination, force_refresh)?;
    fetch_to_file(
        client,
        &format!("{url}.sha256"),
        &checksum_path,
        force_refresh,
    )?;
    fetch_to_file(
        client,
        &format!("{url}.asc"),
        &signature_path,
        force_refresh,
    )?;
    verify_sha256(destination, &checksum_path)?;
    verify_detached_signature(destination, &signature_path, signing_cert)
}

pub(super) fn fetch_indexes(
    client: &Client,
    index_dir: &Path,
    arch: &str,
    force_refresh: bool,
) -> anyhow::Result<()> {
    for repo in ["main", "community"] {
        let url = format!("{BASE_URL}/{repo}/{arch}/APKINDEX.tar.gz");
        let archive_path = index_dir.join(format!("APKINDEX-{repo}.tar.gz"));
        fetch_to_file(client, &url, &archive_path, force_refresh)?;
        let index_path = index_dir.join(format!("APKINDEX-{repo}"));
        extract_member_from_tar_gz(&archive_path, "APKINDEX", &index_path)?;
    }
    Ok(())
}

pub(super) fn fetch_to_file(
    client: &Client,
    url: &str,
    destination: &Path,
    force_refresh: bool,
) -> anyhow::Result<()> {
    if destination.is_file() && !force_refresh && verify_cached_download(destination)? {
        return Ok(());
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    let response = client
        .get(url)
        .send()
        .with_context(|| format!("requesting {url}"))?
        .error_for_status()
        .with_context(|| format!("downloading {url}"))?;
    let bytes = response.bytes().with_context(|| format!("reading {url}"))?;
    fs::write(destination, &bytes).with_context(|| format!("writing {}", destination.display()))?;
    fs::write(
        destination.with_extension("sha256.local"),
        hex_sha256(&bytes),
    )?;
    Ok(())
}

fn verify_cached_download(path: &Path) -> anyhow::Result<bool> {
    let sidecar = path.with_extension("sha256.local");
    if !sidecar.is_file() {
        return Ok(path.metadata().map(|meta| meta.len() > 0).unwrap_or(false));
    }
    let expected = fs::read_to_string(&sidecar)
        .with_context(|| format!("reading {}", sidecar.display()))?
        .trim()
        .to_owned();
    let actual =
        hex_sha256(&fs::read(path).with_context(|| format!("reading {}", path.display()))?);
    Ok(expected == actual)
}
