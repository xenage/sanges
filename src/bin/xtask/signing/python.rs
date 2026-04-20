use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;

pub(super) fn stage_python_binary(package_root: &Path, binary: &Path) -> anyhow::Result<PathBuf> {
    let bin_dir = package_root.join("sagens").join("_bin");
    fs::create_dir_all(&bin_dir).with_context(|| format!("creating {}", bin_dir.display()))?;
    let staged = bin_dir.join("sagens");
    fs::copy(binary, &staged)
        .with_context(|| format!("copying {} to {}", binary.display(), staged.display()))?;
    Ok(staged)
}

#[cfg(test)]
mod tests {
    use super::stage_python_binary;
    use std::fs;

    use tempfile::tempdir;

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn stages_binary_into_python_package_layout() {
        let temp = tempdir().expect("tempdir");
        let package_root = temp.path().join("wheel");
        let source = temp.path().join("sagens");
        fs::write(&source, b"binary").expect("write source");
        #[cfg(unix)]
        fs::set_permissions(&source, fs::Permissions::from_mode(0o755))
            .expect("set source permissions");

        let staged = stage_python_binary(&package_root, &source).expect("stage python binary");

        assert_eq!(
            staged,
            package_root.join("sagens").join("_bin").join("sagens")
        );
        assert_eq!(fs::read(&staged).expect("read staged"), b"binary");
        #[cfg(unix)]
        assert_eq!(
            fs::metadata(&staged)
                .expect("staged metadata")
                .permissions()
                .mode()
                & 0o777,
            0o755
        );
    }
}
