//! Temporary directories and SQLite databases for tests.

use std::fmt;
use std::path::Path;

use tempfile::TempDir;

/// A temporary directory, deleted (with its contents) when dropped.
///
/// Deletion is best effort: on Windows a file still open (a database pool
/// that outlives the guard) keeps the directory until the OS cleans it up.
pub struct TempDirGuard {
    dir: TempDir,
}

impl TempDirGuard {
    /// A new, empty temporary directory.
    ///
    /// # Panics
    ///
    /// When the directory cannot be created.
    pub fn new() -> Self {
        let dir = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .expect("a temporary directory can be created");
        Self { dir }
    }

    /// The directory.
    pub fn path(&self) -> &Path {
        self.dir.path()
    }
}

impl Default for TempDirGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for TempDirGuard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("TempDirGuard").field(&self.path()).finish()
    }
}

/// A navree-db URL for a new SQLite file in a fresh temporary directory
/// (`sqlite://<dir>/database.sqlite?mode=rwc`; SQLite creates the file on
/// first connect), and the guard that deletes the directory.
///
/// Each test gets its own database **file**: an in-memory SQLite database
/// would give every pool connection a different database.
///
/// ```
/// let (url, _guard) = navree_testing::temp_sqlite_url();
/// assert!(url.starts_with("sqlite://") && url.ends_with("?mode=rwc"));
/// ```
pub fn temp_sqlite_url() -> (String, TempDirGuard) {
    let guard = TempDirGuard::new();
    let file = guard.path().join("database.sqlite");
    (format!("sqlite://{}?mode=rwc", file.display()), guard)
}
