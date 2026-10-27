//! Where a test's screenshots and failure files go:
//! `<artifacts>/<test name>/NN-<name>.<ext>`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex};

use crate::BrowserError;

/// The folder of one test and the counter numbering its files.
#[derive(Clone, Debug)]
pub(crate) struct Artifacts {
    dir: PathBuf,
    counter: Arc<AtomicUsize>,
}

/// Test folders already emptied by this process, with their counters.
static FOLDERS: LazyLock<Mutex<HashMap<PathBuf, Arc<AtomicUsize>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

impl Artifacts {
    /// The folder for the test running on this thread under `root`. The
    /// first time a process uses a folder, the files of earlier runs are
    /// deleted.
    pub(crate) fn for_current_test(root: &Path) -> Artifacts {
        let name = std::thread::current()
            .name()
            .map(str::to_owned)
            .unwrap_or_default();
        Artifacts::for_test(root, &name)
    }

    pub(crate) fn for_test(root: &Path, test: &str) -> Artifacts {
        let dir = root.join(test_folder(test));
        let mut folders = FOLDERS.lock().unwrap_or_else(|e| e.into_inner());
        let counter = folders
            .entry(dir.clone())
            .or_insert_with(|| {
                let _ = std::fs::remove_dir_all(&dir);
                Arc::new(AtomicUsize::new(0))
            })
            .clone();
        Artifacts { dir, counter }
    }

    /// The test's folder.
    pub(crate) fn dir(&self) -> &Path {
        &self.dir
    }

    /// The next file number, as two (or more) digits.
    pub(crate) fn next_number(&self) -> String {
        format!("{:02}", self.counter.fetch_add(1, Ordering::SeqCst) + 1)
    }

    /// Writes `bytes` to `<dir>/<file>`, creating the folder.
    pub(crate) fn write(&self, file: &str, bytes: &[u8]) -> Result<PathBuf, BrowserError> {
        std::fs::create_dir_all(&self.dir).map_err(|e| {
            BrowserError::io(format!("cannot create {}", self.dir.display()), e)
        })?;
        let path = self.dir.join(file);
        std::fs::write(&path, bytes)
            .map_err(|e| BrowserError::io(format!("cannot write {}", path.display()), e))?;
        Ok(path)
    }
}

/// A test's thread name as a relative folder: `module::test` becomes
/// `module/test`; other characters outside `[A-Za-z0-9_.-]` become `_`.
/// Threads without a test name use `unnamed`.
pub(crate) fn test_folder(test: &str) -> PathBuf {
    let test = test.trim();
    if test.is_empty() || test == "main" {
        return PathBuf::from("unnamed");
    }
    test.split("::")
        .filter(|part| !part.is_empty())
        .map(|part| {
            part.chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.') {
                        c
                    } else {
                        '_'
                    }
                })
                .collect::<String>()
        })
        .map(|part| if part == ".." || part == "." { "_".to_owned() } else { part })
        .collect()
}

/// A screenshot or file name as given by a test, safe for a file name.
pub(crate) fn file_stem(name: &str) -> String {
    let stem: String = name
        .trim()
        .trim_end_matches(".png")
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.') {
                c
            } else {
                '-'
            }
        })
        .collect();
    if stem.is_empty() {
        "screenshot".to_owned()
    } else {
        stem
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_names_become_folders() {
        assert_eq!(
            test_folder("a_rep_moves_a_deal_to_won"),
            PathBuf::from("a_rep_moves_a_deal_to_won")
        );
        assert_eq!(
            test_folder("actions::types_into_fields"),
            PathBuf::from("actions").join("types_into_fields")
        );
        assert_eq!(test_folder("weird name/..\\x"), PathBuf::from("weird_name_.._x"));
        assert_eq!(test_folder("::.."), PathBuf::from("_"));
        assert_eq!(test_folder(""), PathBuf::from("unnamed"));
        assert_eq!(test_folder("main"), PathBuf::from("unnamed"));
    }

    #[test]
    fn file_names_are_numbered_per_test_and_earlier_runs_are_cleared() {
        let root = tempfile::tempdir().unwrap();
        let stale = root.path().join("numbered").join("01-old.png");
        std::fs::create_dir_all(stale.parent().unwrap()).unwrap();
        std::fs::write(&stale, b"old").unwrap();

        let artifacts = Artifacts::for_test(root.path(), "numbered");
        assert!(!stale.exists(), "files of earlier runs are deleted");
        assert_eq!(artifacts.next_number(), "01");
        // A second browser in the same test shares the numbering.
        let again = Artifacts::for_test(root.path(), "numbered");
        assert_eq!(again.next_number(), "02");
        let path = again.write("02-board.png", b"png").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"png");
        assert_eq!(path, root.path().join("numbered").join("02-board.png"));
        assert_eq!(again.dir(), root.path().join("numbered"));
    }

    #[test]
    fn screenshot_names_are_safe() {
        assert_eq!(file_stem("board-after-move"), "board-after-move");
        assert_eq!(file_stem("board after/move.png"), "board-after-move");
        assert_eq!(file_stem("  "), "screenshot");
    }
}
