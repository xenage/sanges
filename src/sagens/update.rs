#[path = "update/platform.rs"]
mod platform;
#[cfg(test)]
#[path = "update/tests.rs"]
mod tests;

use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use reqwest::Client;
use reqwest::header::{ACCEPT, HeaderMap, HeaderValue, USER_AGENT};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::{Result, SandboxError};
use platform::TargetPlatform;

const DEFAULT_RELEASE_REPO: &str = "xenage/sanges";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelfUpdateOutcome {
    pub release_tag: String,
    pub executable_path: PathBuf,
    pub platform: &'static str,
    pub action: SelfUpdateAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelfUpdateAction {
    AlreadyCurrent,
    Updated,
}

#[derive(Debug, Deserialize)]
struct ReleaseMetadata {
    tag_name: String,
    assets: Vec<ReleaseAsset>,
}

#[derive(Debug, Deserialize)]
struct ReleaseAsset {
    name: String,
    browser_download_url: String,
}

pub async fn run_self_update() -> Result<SelfUpdateOutcome> {
    let executable_path = std::env::current_exe()
        .map_err(|error| SandboxError::io("discovering current sagens binary", error))?;
    let platform = TargetPlatform::detect()?;
    let client = build_http_client()?;
    let release_repo = release_repo();
    let release = fetch_latest_release(&client, &release_repo).await?;
    let (binary_asset, checksum_asset) = select_release_assets(&release, platform)?;
    let checksum_manifest = fetch_text(&client, &checksum_asset.browser_download_url).await?;
    let expected_digest = parse_sha256_manifest(&checksum_manifest, &binary_asset.name)?;

    if hash_file(&executable_path)? == expected_digest {
        return Ok(SelfUpdateOutcome {
            release_tag: release.tag_name,
            executable_path,
            platform: platform.slug(),
            action: SelfUpdateAction::AlreadyCurrent,
        });
    }

    let payload = fetch_bytes(&client, &binary_asset.browser_download_url).await?;
    verify_bytes_sha256(&payload, expected_digest, &binary_asset.name)?;

    let staged_path = allocate_staged_path(&executable_path)?;
    write_staged_binary(&staged_path, &payload, &executable_path)?;
    if let Err(error) = smoke_test_binary(&staged_path).await {
        cleanup_staged_file(&staged_path);
        return Err(error);
    }
    if let Err(error) = replace_binary(&staged_path, &executable_path) {
        cleanup_staged_file(&staged_path);
        return Err(error);
    }

    Ok(SelfUpdateOutcome {
        release_tag: release.tag_name,
        executable_path,
        platform: platform.slug(),
        action: SelfUpdateAction::Updated,
    })
}

fn release_repo() -> String {
    std::env::var("SAGENS_UPDATE_REPO")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            std::env::var("SAGENS_REPO")
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
        .or_else(|| {
            option_env!("SAGENS_RELEASE_REPO")
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .unwrap_or_else(|| DEFAULT_RELEASE_REPO.to_string())
}

fn build_http_client() -> Result<Client> {
    let mut headers = HeaderMap::new();
    headers.insert(
        ACCEPT,
        HeaderValue::from_static("application/vnd.github+json"),
    );
    headers.insert(
        USER_AGENT,
        HeaderValue::from_static(concat!("sanges-self-update/", env!("CARGO_PKG_VERSION"))),
    );
    Client::builder()
        .default_headers(headers)
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|error| {
            SandboxError::backend(format!("building self-update HTTP client: {error}"))
        })
}

async fn fetch_latest_release(client: &Client, release_repo: &str) -> Result<ReleaseMetadata> {
    let url = format!("https://api.github.com/repos/{release_repo}/releases/latest");
    let body = fetch_text(client, &url).await?;
    serde_json::from_str(&body)
        .map_err(|error| SandboxError::backend(format!("parsing GitHub release metadata: {error}")))
}

async fn fetch_text(client: &Client, url: &str) -> Result<String> {
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| SandboxError::backend(format!("downloading {url}: {error}")))?;
    let status = response.status();
    if !status.is_success() {
        return Err(SandboxError::backend(format!(
            "downloading {url} failed with HTTP {status}"
        )));
    }
    response.text().await.map_err(|error| {
        SandboxError::backend(format!("reading response body from {url}: {error}"))
    })
}

async fn fetch_bytes(client: &Client, url: &str) -> Result<Vec<u8>> {
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| SandboxError::backend(format!("downloading {url}: {error}")))?;
    let status = response.status();
    if !status.is_success() {
        return Err(SandboxError::backend(format!(
            "downloading {url} failed with HTTP {status}"
        )));
    }
    response
        .bytes()
        .await
        .map(|bytes| bytes.to_vec())
        .map_err(|error| {
            SandboxError::backend(format!("reading release payload from {url}: {error}"))
        })
}

fn select_release_assets(
    release: &ReleaseMetadata,
    platform: TargetPlatform,
) -> Result<(&ReleaseAsset, &ReleaseAsset)> {
    let binary_name = format!("sagens-{}-{}", release.tag_name, platform.slug());
    let checksum_name = format!("{binary_name}.sha256");
    let binary_asset = release
        .assets
        .iter()
        .find(|asset| asset.name == binary_name)
        .ok_or_else(|| {
            SandboxError::not_found(format!(
                "latest release {} does not include asset {}",
                release.tag_name, binary_name
            ))
        })?;
    let checksum_asset = release
        .assets
        .iter()
        .find(|asset| asset.name == checksum_name)
        .ok_or_else(|| {
            SandboxError::not_found(format!(
                "latest release {} does not include asset {}",
                release.tag_name, checksum_name
            ))
        })?;
    Ok((binary_asset, checksum_asset))
}

fn parse_sha256_manifest(manifest: &str, expected_file_name: &str) -> Result<[u8; 32]> {
    for line in manifest.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut fields = line.split_whitespace();
        let Some(hex) = fields.next() else {
            continue;
        };
        let Some(file_name) = fields.next() else {
            continue;
        };
        if file_name.trim_start_matches('*') != expected_file_name {
            continue;
        }
        return decode_sha256_hex(hex);
    }
    Err(SandboxError::not_found(format!(
        "missing checksum for {}",
        expected_file_name
    )))
}

fn verify_bytes_sha256(bytes: &[u8], expected: [u8; 32], asset_name: &str) -> Result<()> {
    let actual = hash_bytes(bytes);
    if actual == expected {
        return Ok(());
    }
    Err(SandboxError::backend(format!(
        "sha256 mismatch for downloaded release asset {asset_name}"
    )))
}

fn hash_file(path: &Path) -> Result<[u8; 32]> {
    let mut file = std::fs::File::open(path)
        .map_err(|error| SandboxError::io("opening binary for hashing", error))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| SandboxError::io("reading binary for hashing", error))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finalize().into())
}

fn hash_bytes(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize().into()
}

fn decode_sha256_hex(input: &str) -> Result<[u8; 32]> {
    if input.len() != 64 {
        return Err(SandboxError::invalid(format!(
            "invalid sha256 length {}; expected 64 hex characters",
            input.len()
        )));
    }
    let mut bytes = [0_u8; 32];
    for (index, chunk) in input.as_bytes().chunks_exact(2).enumerate() {
        bytes[index] = (decode_hex_nibble(chunk[0])? << 4) | decode_hex_nibble(chunk[1])?;
    }
    Ok(bytes)
}

fn decode_hex_nibble(byte: u8) -> Result<u8> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(SandboxError::invalid(format!(
            "invalid sha256 hex byte {}",
            byte as char
        ))),
    }
}

fn allocate_staged_path(executable_path: &Path) -> Result<PathBuf> {
    let parent = executable_path.parent().ok_or_else(|| {
        SandboxError::invalid(format!(
            "cannot determine parent directory for {}",
            executable_path.display()
        ))
    })?;
    let file_name = executable_path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .ok_or_else(|| {
            SandboxError::invalid(format!(
                "cannot determine file name for {}",
                executable_path.display()
            ))
        })?;
    let pid = std::process::id();
    for attempt in 0..32 {
        let candidate = parent.join(format!(".{file_name}.update-{pid}-{attempt}"));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(SandboxError::backend(format!(
        "unable to allocate staging path near {}",
        executable_path.display()
    )))
}

fn write_staged_binary(staged_path: &Path, bytes: &[u8], executable_path: &Path) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(staged_path)
        .map_err(|error| SandboxError::io("creating staged update binary", error))?;
    file.write_all(bytes)
        .map_err(|error| SandboxError::io("writing staged update binary", error))?;
    file.sync_all()
        .map_err(|error| SandboxError::io("syncing staged update binary", error))?;
    apply_binary_permissions(staged_path, executable_path)?;
    Ok(())
}

#[cfg(unix)]
fn apply_binary_permissions(staged_path: &Path, executable_path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let current_mode = std::fs::metadata(executable_path)
        .map_err(|error| SandboxError::io("reading current binary permissions", error))?
        .permissions()
        .mode();
    let mode = if current_mode & 0o111 == 0 {
        current_mode | 0o755
    } else {
        current_mode
    };
    std::fs::set_permissions(staged_path, std::fs::Permissions::from_mode(mode))
        .map_err(|error| SandboxError::io("setting staged update binary permissions", error))
}

#[cfg(not(unix))]
fn apply_binary_permissions(_: &Path, _: &Path) -> Result<()> {
    Ok(())
}

async fn smoke_test_binary(binary_path: &Path) -> Result<()> {
    let status = tokio::process::Command::new(binary_path)
        .arg("--help")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .map_err(|error| SandboxError::io("running staged update smoke test", error))?;
    if status.success() {
        return Ok(());
    }
    Err(SandboxError::backend(format!(
        "downloaded release failed smoke test with exit status {status}"
    )))
}

fn replace_binary(staged_path: &Path, executable_path: &Path) -> Result<()> {
    std::fs::rename(staged_path, executable_path)
        .map_err(|error| SandboxError::io("replacing current sagens binary", error))
}

fn cleanup_staged_file(path: &Path) {
    let _ = std::fs::remove_file(path);
}
