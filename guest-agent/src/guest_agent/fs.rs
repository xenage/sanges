use std::fmt::Write as _;
use std::io::{Read, Write as _};
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::workspace::{FileKind, FileNode, ReadFileResult, resolve_workspace_path};
use crate::{Result, SandboxError};

const WORKSPACE_ROOT: &str = "/workspace";

pub async fn snapshot_workspace() -> Result<Vec<FileNode>> {
    run_blocking(|| {
        let root = Path::new(WORKSPACE_ROOT);
        let mut entries = Vec::new();
        walk_directory(root, root, &mut entries)?;
        Ok(entries)
    })
    .await
}

pub async fn sync_workspace() -> Result<()> {
    run_blocking(|| {
        unsafe { libc::sync() };
        Ok(())
    })
    .await
}

pub async fn list_files(path: &str) -> Result<Vec<FileNode>> {
    let path = path.to_string();
    run_blocking(move || {
        let root = Path::new(WORKSPACE_ROOT);
        let dir = resolve_workspace_path(root, &path)?;
        let mut entries = std::fs::read_dir(&dir)
            .map_err(|error| SandboxError::io("reading workspace directory", error))?
            .collect::<std::io::Result<Vec<_>>>()
            .map_err(|error| SandboxError::io("iterating workspace directory", error))?;
        entries.sort_by_key(|entry| entry.file_name());
        let mut nodes = Vec::new();
        for entry in entries {
            nodes.push(node_for_path(root, &entry.path())?);
        }
        Ok(nodes)
    })
    .await
}

pub async fn read_file(path: &str, limit: usize) -> Result<ReadFileResult> {
    let path = path.to_string();
    run_blocking(move || {
        let root = Path::new(WORKSPACE_ROOT);
        let file_path = resolve_workspace_path(root, &path)?;
        let mut file = std::fs::File::open(&file_path)
            .map_err(|error| SandboxError::io("opening workspace file", error))?;
        let mut data = Vec::new();
        Read::by_ref(&mut file)
            .take(limit as u64 + 1)
            .read_to_end(&mut data)
            .map_err(|error| SandboxError::io("reading workspace file", error))?;
        let truncated = data.len() > limit;
        if truncated {
            data.truncate(limit);
        }
        Ok(ReadFileResult {
            path,
            data,
            truncated,
        })
    })
    .await
}

pub async fn write_file(path: &str, data: Vec<u8>, create_parents: bool) -> Result<()> {
    let path = path.to_string();
    run_blocking(move || {
        let root = Path::new(WORKSPACE_ROOT);
        let file_path = resolve_workspace_path(root, &path)?;
        if let Some(parent) = file_path.parent()
            && create_parents
        {
            std::fs::create_dir_all(parent)
                .map_err(|error| SandboxError::io("creating workspace parent directory", error))?;
        }
        let mut file = std::fs::File::create(&file_path)
            .map_err(|error| SandboxError::io("creating workspace file", error))?;
        file.write_all(&data)
            .map_err(|error| SandboxError::io("writing workspace file", error))
    })
    .await
}

pub async fn make_dir(path: &str, recursive: bool) -> Result<()> {
    let path = path.to_string();
    run_blocking(move || {
        let root = Path::new(WORKSPACE_ROOT);
        let dir = resolve_workspace_path(root, &path)?;
        if recursive {
            std::fs::create_dir_all(dir)
        } else {
            std::fs::create_dir(dir)
        }
        .map_err(|error| SandboxError::io("creating workspace directory", error))
    })
    .await
}

pub async fn remove_path(path: &str, recursive: bool) -> Result<()> {
    let path = path.to_string();
    run_blocking(move || {
        let root = Path::new(WORKSPACE_ROOT);
        let target = resolve_workspace_path(root, &path)?;
        let metadata = std::fs::symlink_metadata(&target)
            .map_err(|error| SandboxError::io("reading workspace path metadata", error))?;
        if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() {
            if recursive {
                std::fs::remove_dir_all(target)
            } else {
                std::fs::remove_dir(target)
            }
        } else {
            std::fs::remove_file(target)
        }
        .map_err(|error| SandboxError::io("removing workspace path", error))
    })
    .await
}

fn walk_directory(root: &Path, current: &Path, entries: &mut Vec<FileNode>) -> Result<()> {
    if current != root {
        entries.push(FileNode {
            path: relative_path(root, current)?,
            kind: FileKind::Directory,
            size: 0,
            digest: None,
            target: None,
        });
    }
    let mut dir_entries = std::fs::read_dir(current)
        .map_err(|error| SandboxError::io("reading workspace directory", error))?
        .collect::<std::io::Result<Vec<_>>>()
        .map_err(|error| SandboxError::io("iterating workspace directory", error))?;
    dir_entries.sort_by_key(|entry| entry.file_name());
    for entry in dir_entries {
        let path = entry.path();
        let metadata = std::fs::symlink_metadata(&path)
            .map_err(|error| SandboxError::io("reading workspace entry metadata", error))?;
        let file_type = metadata.file_type();
        if file_type.is_dir() && !file_type.is_symlink() {
            walk_directory(root, &path, entries)?;
        } else {
            entries.push(node_for_path(root, &path)?);
        }
    }
    Ok(())
}

fn node_for_path(root: &Path, path: &Path) -> Result<FileNode> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| SandboxError::io("reading workspace path metadata", error))?;
    let kind = if metadata.file_type().is_symlink() {
        FileKind::Symlink
    } else if metadata.file_type().is_dir() {
        FileKind::Directory
    } else {
        FileKind::File
    };
    let (size, digest, target) = match kind {
        FileKind::Directory => (0, None, None),
        FileKind::Symlink => {
            let target = std::fs::read_link(path)
                .map_err(|error| SandboxError::io("reading workspace symlink", error))?;
            let target = target.display().to_string();
            (0, Some(digest_bytes(target.as_bytes())), Some(target))
        }
        FileKind::File => (metadata.len(), Some(digest_file(path)?), None),
    };
    Ok(FileNode {
        path: relative_path(root, path)?,
        kind,
        size,
        digest,
        target,
    })
}

fn digest_file(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path)
        .map_err(|error| SandboxError::io("opening workspace file for hashing", error))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 128 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| SandboxError::io("hashing workspace file", error))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex_encode(hasher.finalize().as_slice()))
}

fn digest_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex_encode(hasher.finalize().as_slice())
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(&mut output, "{byte:02x}");
    }
    output
}

fn relative_path(root: &Path, path: &Path) -> Result<String> {
    path.strip_prefix(root)
        .map_err(|_| SandboxError::backend("workspace path escaped root"))?
        .components()
        .map(|component| component.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("/")
        .pipe(Ok)
}

async fn run_blocking<T, F>(task: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T> + Send + 'static,
{
    tokio::task::spawn_blocking(task).await.map_err(|error| {
        SandboxError::backend(format!("joining blocking workspace task: {error}"))
    })?
}

trait Pipe: Sized {
    fn pipe<T>(self, f: impl FnOnce(Self) -> T) -> T {
        f(self)
    }
}

impl<T> Pipe for T {}
