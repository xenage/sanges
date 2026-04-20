use std::path::{Path, PathBuf};

use tokio::fs;

use crate::SandboxError;
pub use crate::box_api::BoxApiClient as SagensClient;
use crate::workspace::FileKind;

pub async fn upload_path(
    client: &SagensClient,
    box_id: uuid::Uuid,
    local_path: &Path,
    remote_path: &Path,
) -> crate::Result<()> {
    let metadata = fs::metadata(local_path)
        .await
        .map_err(|error| crate::SandboxError::io("reading local upload path metadata", error))?;
    if metadata.is_dir() {
        client
            .make_dir(box_id, remote_path.display().to_string(), true)
            .await?;
        upload_directory(client, box_id, local_path, remote_path).await
    } else {
        client
            .write_file(
                box_id,
                remote_path.display().to_string(),
                fs::read(local_path)
                    .await
                    .map_err(|error| crate::SandboxError::io("reading local upload file", error))?,
                true,
            )
            .await
    }
}

async fn upload_directory(
    client: &SagensClient,
    box_id: uuid::Uuid,
    local_dir: &Path,
    remote_dir: &Path,
) -> crate::Result<()> {
    let mut entries = fs::read_dir(local_dir)
        .await
        .map_err(|error| crate::SandboxError::io("reading local upload directory", error))?;
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|error| crate::SandboxError::io("iterating local upload directory", error))?
    {
        let local_path = entry.path();
        let remote_path = remote_dir.join(entry.file_name());
        let metadata = entry.metadata().await.map_err(|error| {
            crate::SandboxError::io("reading local upload entry metadata", error)
        })?;
        if metadata.is_dir() {
            client
                .make_dir(box_id, remote_path.display().to_string(), true)
                .await?;
            Box::pin(upload_directory(client, box_id, &local_path, &remote_path)).await?;
        } else {
            client
                .write_file(
                    box_id,
                    remote_path.display().to_string(),
                    fs::read(&local_path).await.map_err(|error| {
                        crate::SandboxError::io("reading local upload file", error)
                    })?,
                    true,
                )
                .await?;
        }
    }
    Ok(())
}

pub async fn download_path(
    client: &SagensClient,
    box_id: uuid::Uuid,
    remote_path: &str,
    local_path: &Path,
) -> crate::Result<()> {
    match client.list_files(box_id, remote_path.into()).await {
        Ok(entries) => {
            fs::create_dir_all(local_path).await.map_err(|error| {
                crate::SandboxError::io("creating local download directory", error)
            })?;
            download_directory(client, box_id, local_path, &entries).await
        }
        Err(error) if should_fallback_to_file_download(&error) => {
            let file = client
                .read_file(box_id, remote_path.into(), 16 * 1024 * 1024)
                .await?;
            write_download_file(
                &resolve_download_file_path(remote_path, local_path),
                &file.data,
            )
            .await
        }
        Err(error) => Err(error),
    }
}

async fn download_directory(
    client: &SagensClient,
    box_id: uuid::Uuid,
    local_root: &Path,
    entries: &[crate::workspace::FileNode],
) -> crate::Result<()> {
    for entry in entries {
        let relative = PathBuf::from(&entry.path);
        let local_path = local_root.join(&relative);
        match entry.kind {
            FileKind::Directory => {
                fs::create_dir_all(&local_path).await.map_err(|error| {
                    crate::SandboxError::io("creating local downloaded directory", error)
                })?;
            }
            FileKind::File => {
                let remote_path = format!("/workspace/{}", entry.path);
                let file = client
                    .read_file(box_id, remote_path, 16 * 1024 * 1024)
                    .await?;
                write_download_file(&local_path, &file.data).await?;
            }
            FileKind::Symlink => {
                return Err(crate::SandboxError::invalid(format!(
                    "downloading symlinks is not supported yet: {}",
                    entry.path
                )));
            }
        }
    }
    Ok(())
}

async fn write_download_file(path: &Path, data: &[u8]) -> crate::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await.map_err(|error| {
            crate::SandboxError::io("creating local download parent directory", error)
        })?;
    }
    fs::write(path, data)
        .await
        .map_err(|error| crate::SandboxError::io("writing local downloaded file", error))
}

fn should_fallback_to_file_download(error: &SandboxError) -> bool {
    match error {
        SandboxError::Io { source, .. } => {
            matches!(source.kind(), std::io::ErrorKind::NotADirectory)
                || matches!(source.raw_os_error(), Some(libc::ENOTDIR))
        }
        SandboxError::Backend(message) | SandboxError::Protocol(message) => {
            message.contains("reading workspace directory")
                && (message.contains("Not a directory") || message.contains("os error 20"))
        }
        _ => false,
    }
}

fn resolve_download_file_path(remote_path: &str, local_path: &Path) -> PathBuf {
    if local_path.is_dir() {
        return local_path.join(remote_file_name(remote_path));
    }
    if remote_path.ends_with('/') {
        return local_path.to_path_buf();
    }
    if local_path
        .file_name()
        .map(|name| name == "." || name == "..")
        .unwrap_or(false)
    {
        return local_path.join(remote_file_name(remote_path));
    }
    local_path.to_path_buf()
}

fn remote_file_name(remote_path: &str) -> &str {
    if remote_path.ends_with('/') {
        return "downloaded-file";
    }
    let trimmed = remote_path.trim_end_matches('/');
    if trimmed.is_empty() {
        return "downloaded-file";
    }
    Path::new(trimmed)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("downloaded-file")
}

#[cfg(test)]
mod tests {
    use std::io;

    use tempfile::tempdir;

    use super::{remote_file_name, resolve_download_file_path, should_fallback_to_file_download};
    use crate::SandboxError;

    #[test]
    fn falls_back_only_for_not_a_directory_errors() {
        let enotdir = SandboxError::io(
            "reading workspace directory",
            io::Error::from_raw_os_error(libc::ENOTDIR),
        );
        let wrapped_enotdir = SandboxError::backend(
            "protocol error: reading workspace directory: Not a directory (os error 20)",
        );
        let not_found = SandboxError::io(
            "reading workspace directory",
            io::Error::from(io::ErrorKind::NotFound),
        );
        let backend = SandboxError::backend("websocket connection closed");

        assert!(should_fallback_to_file_download(&enotdir));
        assert!(should_fallback_to_file_download(&wrapped_enotdir));
        assert!(!should_fallback_to_file_download(&not_found));
        assert!(!should_fallback_to_file_download(&backend));
    }

    #[test]
    fn resolves_download_into_existing_directory() {
        let temp = tempdir().expect("tempdir");
        let destination = resolve_download_file_path("/workspace/jojo", temp.path());

        assert_eq!(destination, temp.path().join("jojo"));
    }

    #[test]
    fn keeps_explicit_file_destination() {
        let temp = tempdir().expect("tempdir");
        let destination =
            resolve_download_file_path("/workspace/jojo", &temp.path().join("renamed.txt"));

        assert_eq!(destination, temp.path().join("renamed.txt"));
    }

    #[test]
    fn extracts_remote_file_name_safely() {
        assert_eq!(remote_file_name("/workspace/jojo"), "jojo");
        assert_eq!(remote_file_name("jojo"), "jojo");
        assert_eq!(remote_file_name("/workspace/"), "downloaded-file");
    }
}
