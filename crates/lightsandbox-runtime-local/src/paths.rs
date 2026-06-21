use std::path::{Path, PathBuf};

use lightsandbox_core::LightSandboxError;

/// Resolves a client-supplied path against a sandbox's workspace root,
/// rejecting traversal (`..`) and (unless explicitly allowed) absolute paths.
/// Built by manually walking path components rather than `Path::join`, so an
/// attacker-controlled string can never escape `workspace_root`.
pub fn safe_path(
    workspace_root: &Path,
    requested: &str,
    allow_absolute: bool,
) -> Result<PathBuf, LightSandboxError> {
    if requested.trim().is_empty() {
        return Err(LightSandboxError::InvalidPath(
            "path must not be empty".into(),
        ));
    }

    let bytes = requested.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' {
        return Err(LightSandboxError::InvalidPath(
            "drive-letter absolute paths are not allowed".into(),
        ));
    }

    let normalized = requested.replace('\\', "/");
    let rest = if normalized.starts_with('/') {
        if !allow_absolute {
            return Err(LightSandboxError::InvalidPath(
                "absolute paths are not allowed".into(),
            ));
        }
        normalized.trim_start_matches('/')
    } else {
        normalized.as_str()
    };

    let mut result = workspace_root.to_path_buf();
    for component in rest.split('/') {
        if component.is_empty() || component == "." {
            continue;
        }
        if component == ".." {
            return Err(LightSandboxError::InvalidPath(
                "path traversal is not allowed".into(),
            ));
        }
        result.push(component);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_parent_traversal() {
        let root = PathBuf::from("/workspace/sbx_1");
        assert!(safe_path(&root, "../escape.txt", false).is_err());
        assert!(safe_path(&root, "a/../../escape.txt", false).is_err());
    }

    #[test]
    fn rejects_absolute_by_default() {
        let root = PathBuf::from("/workspace/sbx_1");
        assert!(safe_path(&root, "/etc/passwd", false).is_err());
        assert!(safe_path(&root, "C:\\Windows\\System32", false).is_err());
    }

    #[test]
    fn allows_nested_relative_path() {
        let root = PathBuf::from("/workspace/sbx_1");
        let resolved = safe_path(&root, "a/b/c.txt", false).unwrap();
        assert_eq!(resolved, root.join("a").join("b").join("c.txt"));
    }
}
