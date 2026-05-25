use std::fs::{File, OpenOptions};
use std::io::Write as _;
use std::path::Path;

#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

use crate::{Result, SandboxError};

pub(crate) fn ensure_private_dir(path: &Path, context: &str) -> Result<()> {
    std::fs::create_dir_all(path).map_err(|error| SandboxError::io(context, error))?;
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| SandboxError::io(format!("reading {}", path.display()), error))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(SandboxError::invalid(format!(
            "{} must be a real directory: {}",
            context,
            path.display()
        )));
    }
    #[cfg(unix)]
    {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .map_err(|error| SandboxError::io(format!("setting {}", path.display()), error))?;
    }
    Ok(())
}

pub(crate) fn open_private_file_truncate(
    path: &Path,
    dir_context: &str,
    file_context: &str,
) -> Result<File> {
    if let Some(parent) = path.parent() {
        ensure_private_dir(parent, dir_context)?;
    }
    let mut options = OpenOptions::new();
    options.create(true).write(true).truncate(true);
    #[cfg(unix)]
    {
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let file = options
        .open(path)
        .map_err(|error| SandboxError::io(file_context, error))?;
    set_private_file_permissions(path, file_context)?;
    Ok(file)
}

pub(crate) fn write_private_file(
    path: &Path,
    bytes: &[u8],
    dir_context: &str,
    file_context: &str,
) -> Result<()> {
    let mut file = open_private_file_truncate(path, dir_context, file_context)?;
    file.write_all(bytes)
        .map_err(|error| SandboxError::io(file_context, error))?;
    file.sync_all()
        .map_err(|error| SandboxError::io(format!("syncing {}", path.display()), error))
}

pub(crate) fn validate_private_file_permissions(path: &Path, context: &str) -> Result<()> {
    #[cfg(unix)]
    {
        let metadata = std::fs::metadata(path)
            .map_err(|error| SandboxError::io(format!("reading {context} metadata"), error))?;
        let mode = metadata.permissions().mode() & 0o777;
        if mode & 0o077 != 0 {
            return Err(SandboxError::invalid(format!(
                "{context} {} must have 0600 permissions",
                path.display()
            )));
        }
    }
    Ok(())
}

fn set_private_file_permissions(path: &Path, context: &str) -> Result<()> {
    #[cfg(unix)]
    {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .map_err(|error| SandboxError::io(format!("setting {context} permissions"), error))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::{ensure_private_dir, open_private_file_truncate};

    #[cfg(unix)]
    #[test]
    fn creates_private_directory_and_file() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempdir().expect("tempdir");
        let dir = temp.path().join("state");
        ensure_private_dir(&dir, "creating test state directory").expect("private dir");

        let mode = std::fs::metadata(&dir)
            .expect("dir metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o700);

        let path = dir.join("secret.txt");
        let _ = open_private_file_truncate(
            &path,
            "creating test secret directory",
            "opening test secret file",
        )
        .expect("private file");
        let mode = std::fs::metadata(&path)
            .expect("file metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }
}
