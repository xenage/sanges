use std::path::{Component, Path, PathBuf};

use crate::{Result, SandboxError};

pub fn normalize_workspace_path(path: &str) -> Result<String> {
    if path.is_empty() || path == "." || path == "/workspace" {
        return Ok("/workspace".into());
    }

    let candidate = if path.starts_with("/workspace/") {
        path.to_string()
    } else if path.starts_with('/') {
        return Err(SandboxError::invalid(
            "filesystem access is restricted to /workspace",
        ));
    } else {
        format!("/workspace/{}", path.trim_start_matches('/'))
    };

    validate_relative(Path::new(candidate.trim_start_matches("/workspace/")))?;
    Ok(candidate)
}

pub fn resolve_workspace_path(root: &Path, path: &str) -> Result<PathBuf> {
    let normalized = normalize_workspace_path(path)?;
    if normalized == "/workspace" {
        return Ok(root.to_path_buf());
    }
    let relative = Path::new(normalized.trim_start_matches("/workspace/"));
    validate_relative(relative)?;
    Ok(root.join(relative))
}

pub fn validate_persisted_id(value: &str, field: &str) -> Result<()> {
    if value.is_empty() {
        return Err(SandboxError::invalid(format!("{field} must not be empty")));
    }
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        return Ok(());
    }
    Err(SandboxError::invalid(format!(
        "{field} may only contain ASCII letters, digits, '-' or '_'"
    )))
}

fn validate_relative(path: &Path) -> Result<()> {
    for component in path.components() {
        match component {
            Component::Normal(_) => {}
            Component::CurDir => {}
            _ => {
                return Err(SandboxError::invalid(
                    "workspace path must stay within /workspace",
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::normalize_workspace_path;

    #[test]
    fn rejects_escape_attempts() {
        assert!(normalize_workspace_path("../nope").is_err());
        assert!(normalize_workspace_path("/etc/passwd").is_err());
        assert!(normalize_workspace_path("/workspace/../nope").is_err());
    }
}
