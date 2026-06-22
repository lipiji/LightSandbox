use std::path::{Path, PathBuf};

use lightsandbox_core::LightSandboxError;

/// Resolves a client-supplied path against a sandbox's workspace root,
/// rejecting traversal (`..`) and (unless explicitly allowed) absolute paths.
/// Built by manually walking path components rather than `Path::join`, so an
/// attacker-controlled string can never escape `workspace_root` unless the
/// caller explicitly opts into `allow_traversal`.
pub fn safe_path(
    workspace_root: &Path,
    requested: &str,
    allow_absolute: bool,
    allow_traversal: bool,
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
            if allow_traversal {
                result.push("..");
                continue;
            }
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
        assert!(safe_path(&root, "../escape.txt", false, false).is_err());
        assert!(safe_path(&root, "a/../../escape.txt", false, false).is_err());
    }

    #[test]
    fn rejects_absolute_by_default() {
        let root = PathBuf::from("/workspace/sbx_1");
        assert!(safe_path(&root, "/etc/passwd", false, false).is_err());
        assert!(safe_path(&root, "C:\\Windows\\System32", false, false).is_err());
    }

    #[test]
    fn allows_nested_relative_path() {
        let root = PathBuf::from("/workspace/sbx_1");
        let resolved = safe_path(&root, "a/b/c.txt", false, false).unwrap();
        assert_eq!(resolved, root.join("a").join("b").join("c.txt"));
    }

    #[test]
    fn allows_traversal_when_explicitly_enabled() {
        let root = PathBuf::from("/workspace/sbx_1");
        let resolved = safe_path(&root, "../escape.txt", false, true).unwrap();
        assert_eq!(resolved, root.join("..").join("escape.txt"));
    }

    #[test]
    fn rejects_empty_and_whitespace_only_path() {
        let root = PathBuf::from("/workspace/sbx_1");
        assert!(safe_path(&root, "", false, false).is_err());
        assert!(safe_path(&root, "   ", false, false).is_err());
        assert!(safe_path(&root, "\t\n", false, false).is_err());
    }

    #[test]
    fn rejects_drive_letter_path() {
        // `C:` / `D:` style drive letters are caught by the second-byte colon
        // check before any normalization runs, so they can't reach the
        // workspace as an absolute Windows path.
        let root = PathBuf::from("/workspace/sbx_1");
        assert!(safe_path(&root, "C:file.txt", false, false).is_err());
        assert!(safe_path(&root, "D:stuff", false, false).is_err());
    }

    #[test]
    fn normalizes_backslashes_to_slashes() {
        // A Windows-style relative path with backslashes must resolve the same
        // as its forward-slash form — otherwise `a\b\c.txt` would be treated
        // as a single literal filename on Unix workspace roots.
        let root = PathBuf::from("/workspace/sbx_1");
        let resolved = safe_path(&root, "a\\b\\c.txt", false, false).unwrap();
        assert_eq!(resolved, root.join("a").join("b").join("c.txt"));
    }

    #[test]
    fn collapses_dot_and_empty_segments() {
        let root = PathBuf::from("/workspace/sbx_1");
        let resolved = safe_path(&root, "./a//b/./c.txt", false, false).unwrap();
        assert_eq!(resolved, root.join("a").join("b").join("c.txt"));

        // A bare `.` resolves to the workspace root itself (no segments kept).
        assert_eq!(safe_path(&root, ".", false, false).unwrap(), root);
    }

    #[test]
    fn strips_leading_slash_when_absolute_allowed() {
        // With allow_absolute=true, a leading slash is treated as
        // workspace-root-relative (stripped), not host-root-relative.
        let root = PathBuf::from("/workspace/sbx_1");
        let resolved = safe_path(&root, "/x/y.txt", true, false).unwrap();
        assert_eq!(resolved, root.join("x").join("y.txt"));
    }
}
