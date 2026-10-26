//! Helpers shared by the view tests.

use std::fs;
use std::path::Path;

use navis_view::ViewOptions;
use tempfile::TempDir;

/// Writes `source` to `<dir>/<name>`, creating parent directories.
pub fn write(dir: &Path, name: &str, source: &str) {
    let path = dir.join(name);
    fs::create_dir_all(path.parent().expect("template path has a parent")).expect("create dirs");
    fs::write(path, source).expect("write template");
}

/// A temporary views directory holding `templates` (name, source).
pub fn views_dir(templates: &[(&str, &str)]) -> TempDir {
    let dir = tempfile::tempdir().expect("temp dir");
    for (name, source) in templates {
        write(dir.path(), name, source);
    }
    dir
}

/// Options reading only from `dirs`, without reloading.
pub fn options(dirs: &[&Path]) -> ViewOptions {
    ViewOptions {
        dirs: dirs.iter().map(|dir| dir.to_path_buf()).collect(),
        reload: false,
    }
}
