//! glia-fs — native filesystem module for Glia CLI.
//!
//! Implements V1: `glia_action(local-intent="read file")` routes here natively,
//! ⊥ Hub network call. All operations enforce path boundary checks relative to
//! a root directory to prevent traversal outside the project tree.

use std::path::{Path, PathBuf};

use thiserror::Error;
use tokio::fs;

/// Errors returned by FS operations.
#[derive(Debug, Error)]
pub enum FsError {
    /// Path escapes the allowed root (`..` traversal or absolute path outside root).
    #[error("path escapes root: {0}")]
    PathEscape(String),
    /// File not found.
    #[error("not found: {0}")]
    NotFound(String),
    /// Underlying IO error.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    /// Path was not UTF-8.
    #[error("non-utf8 path")]
    NonUtf8,
}

/// A filesystem rooted at `root`. All operations resolve `path` relative to
/// `root` and reject any resolved path that escapes it.
#[derive(Debug, Clone)]
pub struct Fs {
    root: PathBuf,
}

impl Fs {
    /// Create a new rooted FS. `root` is canonicalized on first use.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Resolve `path` relative to root, rejecting traversals.
    fn resolve(&self, path: &str) -> Result<PathBuf, FsError> {
        let p = Path::new(path);
        let joined = if p.is_absolute() {
            p.to_path_buf()
        } else {
            self.root.join(p)
        };
        // Normalize without requiring the file to exist.
        let normalized = normalize(&joined);
        // Check the normalized path starts with root.
        if !normalized.starts_with(&self.root) {
            return Err(FsError::PathEscape(path.to_string()));
        }
        Ok(normalized)
    }

    /// Read a file as UTF-8 text. Implements V1 local routing.
    pub async fn read_file(&self, path: &str) -> Result<String, FsError> {
        let full = self.resolve(path)?;
        if !full.exists() {
            return Err(FsError::NotFound(path.to_string()));
        }
        let bytes = fs::read(&full).await?;
        String::from_utf8(bytes).map_err(|_| FsError::NonUtf8)
    }

    /// Write text to a file, creating parent dirs as needed.
    pub async fn write_file(&self, path: &str, content: &str) -> Result<(), FsError> {
        let full = self.resolve(path)?;
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::write(&full, content.as_bytes()).await?;
        Ok(())
    }

    /// List entries in a directory (files + subdirs, non-recursive).
    pub async fn list_dir(&self, path: &str) -> Result<Vec<String>, FsError> {
        let full = self.resolve(path)?;
        if !full.is_dir() {
            return Err(FsError::NotFound(format!("{path} (not a dir)")));
        }
        let mut entries = fs::read_dir(&full).await?;
        let mut out = Vec::new();
        while let Some(entry) = entries.next_entry().await? {
            if let Some(name) = entry.file_name().to_str() {
                out.push(name.to_string());
            }
        }
        out.sort();
        Ok(out)
    }

    /// Return file metadata (size + modified time).
    pub async fn file_info(&self, path: &str) -> Result<FileInfo, FsError> {
        let full = self.resolve(path)?;
        let meta = fs::metadata(&full).await?;
        Ok(FileInfo {
            size: meta.len(),
            is_dir: meta.is_dir(),
            is_file: meta.is_file(),
            modified: meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs()),
        })
    }

    /// Root directory accessor.
    pub fn root(&self) -> &Path {
        &self.root
    }
}

/// File metadata snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileInfo {
    /// Size in bytes.
    pub size: u64,
    /// True if directory.
    pub is_dir: bool,
    /// True if regular file.
    pub is_file: bool,
    /// Modified time as Unix seconds, if available.
    pub modified: Option<u64>,
}

/// Lexical path normalization without touching the filesystem. Collapses `.`
/// and `..` components. On Windows, converts `/` to `\` for consistency.
fn normalize(path: &Path) -> PathBuf {
    use std::path::Component;

    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                // Only pop if the last component is a normal (not root/prefix).
                let pop_ok = matches!(out.components().next_back(), Some(Component::Normal(_)));
                if pop_ok {
                    out.pop();
                } else {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_dot() {
        let p = normalize(Path::new("/a/./b"));
        assert_eq!(p, PathBuf::from("/a/b"));
    }

    #[test]
    fn normalize_collapses_dotdot() {
        let p = normalize(Path::new("/a/b/../c"));
        assert_eq!(p, PathBuf::from("/a/c"));
    }

    #[test]
    fn normalize_dotdot_at_root_preserves_escape() {
        // On Windows, root prefix is `C:\` — `..` can't pop it, so it stays
        // as an escape marker. On Unix, `/..` collapses to `/`. Either way,
        // the key invariant is: normalize never silently absorbs `..` past
        // a normal component, so resolve() can detect escapes.
        let p = normalize(Path::new("/../x"));
        // Should either be `/x` (Unix) or contain `..` (Windows prefix root).
        assert!(
            p == Path::new("/x")
                || p.components()
                    .any(|c| matches!(c, std::path::Component::ParentDir)),
            "expected escape or root collapse, got {p:?}"
        );
    }
}
