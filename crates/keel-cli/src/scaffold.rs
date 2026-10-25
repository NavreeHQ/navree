//! Generated files: a [`FileSet`] is planned in memory (so it can be tested
//! and shown), then written without overwriting anything.

use std::fmt;
use std::path::{Path, PathBuf};

/// One generated file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedFile {
    /// The path relative to the directory the set is written to, with `/`
    /// separators on every platform.
    pub path: String,
    /// The contents (LF line endings).
    pub contents: String,
}

/// Files to create, in creation order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileSet {
    files: Vec<GeneratedFile>,
}

/// Writing a [`FileSet`] failed.
#[derive(Debug, thiserror::Error)]
pub enum ScaffoldError {
    /// Some files already exist; nothing was written.
    #[error("refusing to overwrite existing files (nothing was written):\n{}", list(.0))]
    Exists(Vec<PathBuf>),
    /// A file could not be written.
    #[error("cannot write {}: {source}", path.display())]
    Io {
        /// The file.
        path: PathBuf,
        /// Why.
        #[source]
        source: std::io::Error,
    },
}

fn list(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|p| format!("  {}", p.display()))
        .collect::<Vec<_>>()
        .join("\n")
}

impl FileSet {
    /// An empty set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a file (`path` uses `/` separators).
    pub fn add(&mut self, path: impl Into<String>, contents: impl Into<String>) {
        self.files.push(GeneratedFile {
            path: path.into(),
            contents: contents.into(),
        });
    }

    /// The paths, in creation order.
    pub fn paths(&self) -> Vec<&str> {
        self.files.iter().map(|f| f.path.as_str()).collect()
    }

    /// The contents of `path`.
    pub fn get(&self, path: &str) -> Option<&str> {
        self.files
            .iter()
            .find(|f| f.path == path)
            .map(|f| f.contents.as_str())
    }

    /// The files.
    pub fn iter(&self) -> impl Iterator<Item = &GeneratedFile> {
        self.files.iter()
    }

    /// Whether the set is empty.
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// Writes every file under `root`, creating directories as needed. When
    /// any file already exists nothing is written.
    pub fn write(&self, root: &Path) -> Result<Vec<PathBuf>, ScaffoldError> {
        let targets: Vec<PathBuf> = self.files.iter().map(|f| join(root, &f.path)).collect();
        let existing: Vec<PathBuf> = targets.iter().filter(|p| p.exists()).cloned().collect();
        if !existing.is_empty() {
            return Err(ScaffoldError::Exists(existing));
        }
        for (file, target) in self.files.iter().zip(&targets) {
            let io = |source| ScaffoldError::Io {
                path: target.clone(),
                source,
            };
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(io)?;
            }
            std::fs::write(target, &file.contents).map_err(io)?;
        }
        Ok(targets)
    }
}

/// `root` joined with a `/`-separated relative path.
pub fn join(root: &Path, relative: &str) -> PathBuf {
    relative
        .split('/')
        .filter(|part| !part.is_empty())
        .fold(root.to_path_buf(), |path, part| path.join(part))
}

/// Every file, each under a `==> path <==` header (for snapshots and
/// `--dry-run`).
impl fmt::Display for FileSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, file) in self.files.iter().enumerate() {
            if i > 0 {
                writeln!(f)?;
            }
            writeln!(f, "==> {} <==", file.path)?;
            f.write_str(&file.contents)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_files_and_directories() {
        let dir = tempfile::tempdir().unwrap();
        let mut set = FileSet::new();
        set.add("a/b/c.rs", "fn main() {}\n");
        set.add("top.txt", "hi\n");
        let written = set.write(dir.path()).unwrap();
        assert_eq!(written.len(), 2);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("a").join("b").join("c.rs")).unwrap(),
            "fn main() {}\n"
        );
    }

    #[test]
    fn never_overwrites_and_writes_nothing_on_conflict() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("taken.rs"), "mine").unwrap();
        let mut set = FileSet::new();
        set.add("new.rs", "new");
        set.add("taken.rs", "theirs");
        let error = set.write(dir.path()).unwrap_err();
        assert!(
            matches!(error, ScaffoldError::Exists(ref p) if p.len() == 1),
            "{error}"
        );
        assert!(!dir.path().join("new.rs").exists());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("taken.rs")).unwrap(),
            "mine"
        );
    }
}
